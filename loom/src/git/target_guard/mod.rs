//! The target guard: detecting moves of a target branch that loom did not make.
//!
//! A stage session can move `refs/heads/<target>` itself, so a change can
//! reach the target without the merge gate. Loom records the last tip it
//! accepted per target in [`RECORD_FILE`] (in the state directory, which no
//! session can write) and evaluates every other tip it observes: a history
//! rewrite, a control-path change, an unattested change outside
//! `doc/loom/knowledge/` (attestation: the ledger loom's
//! `reference-transaction` hook appends to) or unmerged stage work holds the
//! target, and merges into it wait until the operator accepts or restores it.
//! Any other move is accepted silently.

mod attestation;
mod evaluate;
mod record;
mod text;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub use attestation::{append_attestation, attestation_mode, AttestationMode};
pub(crate) use record::knowledge_prefix;
pub use record::{
    accepted_tip, attestation_latched, guarded_refs, record_advance, recorded_hold, recorded_holds,
    recorded_targets,
};
pub(crate) use text::short;
pub use text::{accept_command, hold_alert, restore_commands};

use crate::git::branch::{branch_ref, is_ancestor_of};
use crate::git::merge::lock::MergeLock;
use crate::git::merge::{rev_parse, verify_merge_succeeded};
use attestation::create_ledger;
use evaluate::{evaluate, one_line, Scope};
use record::{read_record, read_record_for_accept, write_record, GuardRecord, TargetEntry};

/// The record of accepted tips, holds and attestation latches, in the state
/// directory.
pub const RECORD_FILE: &str = "target-guard.json";
/// The refs the hook guards, rewritten with every record write.
pub const REFS_FILE: &str = "target-guard.refs";
/// The append-only attestation ledger the hook writes on the host.
pub const LEDGER_FILE: &str = "target-guard.ledger";
/// The marker that identifies loom's `reference-transaction` hook.
pub const HOOK_MARKER: &str = "LOOM_REFERENCE_TRANSACTION_HOOK";

/// How long [`accept`] waits for the merge lock.
const ACCEPT_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// A move of the target that loom did not accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    /// The last tip loom accepted; empty when the record could not be read.
    pub accepted: String,
    /// The tip the hold was recorded for.
    pub observed: String,
    pub reasons: Vec<HoldReason>,
    /// When `observed` was first held.
    pub since: DateTime<Utc>,
}

/// Why a move is held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoldReason {
    /// The observed tip does not descend from the accepted one.
    NotFastForward,
    /// The move changes control paths.
    ControlPaths { paths: Vec<String> },
    /// Attestation is required and no ledger line covers `from`..`to`, which
    /// changes `paths` outside `doc/loom/knowledge/` (at most 20 named).
    Unattested {
        from: String,
        to: String,
        paths: Vec<String>,
    },
    /// The move carries commits of these `loom/*` branches loom never
    /// accepted.
    StageWork { branches: Vec<String> },
    /// The move could not be evaluated.
    Unevaluable { error: String },
}

/// The guard's verdict on a target's current tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardState {
    /// The tip is accepted.
    Clear {
        accepted: String,
    },
    Held(Hold),
}

/// What [`accept`] recorded: the accepted tip moved from `from` to `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub from: String,
    pub to: String,
}

/// `target` without a leading `refs/heads/`: the record's key.
pub(crate) fn target_key(target: &str) -> &str {
    target.strip_prefix("refs/heads/").unwrap_or(target)
}

/// An object id cut to 12 characters, for operator text and log lines.
pub(crate) fn abbrev(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

/// The commit `refs/heads/<key>` points at.
fn target_tip(repo_root: &Path, key: &str) -> Result<String> {
    rev_parse(repo_root, &branch_ref(key))
}

/// The guard's verdict on `target`'s current tip, taking the merge lock
/// in `work_dir` only when the record does not already accept the tip
/// without a hold. `None` while another owner holds the merge lock.
pub fn check(repo_root: &Path, work_dir: &Path, target: &str) -> Result<Option<GuardState>> {
    let key = target_key(target);
    let tip = target_tip(repo_root, key)?;
    let settled = read_record(work_dir)
        .ok()
        .and_then(|mut record| record.targets.remove(key))
        .is_some_and(|entry| entry.accepted == tip && entry.hold.is_none());
    if settled {
        return Ok(Some(GuardState::Clear { accepted: tip }));
    }
    let Some(lock) = MergeLock::try_acquire_in(work_dir)? else {
        return Ok(None);
    };
    let tip = target_tip(repo_root, key)?;
    let state = check_locked(repo_root, work_dir, key, &tip)?;
    lock.release()?;
    Ok(Some(state))
}

/// The guard's verdict on `tip` for `target`, recorded. The caller holds the
/// merge lock.
///
/// No entry: `tip` is trusted and recorded. `tip` is the accepted tip: any
/// hold is cleared. A hold already recorded for `tip` is returned without git
/// work unless it could not be evaluated. Otherwise the move is evaluated: no
/// reason accepts it, any reason records a hold. A record that cannot be read
/// is never overwritten here: the target is held as unevaluable.
pub fn check_locked(
    repo_root: &Path,
    work_dir: &Path,
    target: &str,
    tip: &str,
) -> Result<GuardState> {
    let key = target_key(target);
    let mut record = match read_record(work_dir) {
        Ok(record) => record,
        Err(error) => return Ok(GuardState::Held(unreadable_hold(tip, &error))),
    };
    let Some(entry) = record.targets.get(key).cloned() else {
        return trust_on_first_use(repo_root, work_dir, record, key, tip);
    };
    let (state, updated) = match judge(repo_root, work_dir, key, &entry, tip) {
        Verdict::Accepted => {
            let cleared = TargetEntry {
                accepted: entry.accepted.clone(),
                hold: None,
                attestation: entry.attestation,
            };
            (
                GuardState::Clear {
                    accepted: tip.to_string(),
                },
                cleared,
            )
        }
        Verdict::Memo(hold) => return Ok(GuardState::Held(hold)),
        Verdict::Evaluated { required, reasons } => settle(key, &entry, tip, required, reasons),
    };
    if updated != entry {
        record.targets.insert(key.to_string(), updated);
        write_record(work_dir, &record)?;
    }
    Ok(state)
}

/// What [`judge`] decided about a tip.
enum Verdict {
    /// The tip is the accepted tip.
    Accepted,
    /// The recorded hold already covers the tip.
    Memo(Hold),
    /// A fresh evaluation; no reasons accept the move. `required` is whether
    /// the ledger was walked.
    Evaluated {
        required: bool,
        reasons: Vec<HoldReason>,
    },
}

/// Judge `tip` against `entry` without writing anything. Attestation is
/// required while the mode is `Active` or the entry's latch is set.
fn judge(repo_root: &Path, work_dir: &Path, key: &str, entry: &TargetEntry, tip: &str) -> Verdict {
    if entry.accepted == tip {
        return Verdict::Accepted;
    }
    if let Some(hold) = &entry.hold {
        let unevaluable = hold
            .reasons
            .iter()
            .any(|reason| matches!(reason, HoldReason::Unevaluable { .. }));
        if hold.observed == tip && !unevaluable {
            return Verdict::Memo(hold.clone());
        }
    }
    let required =
        attestation_mode(repo_root, work_dir) == AttestationMode::Active || entry.attestation;
    let scope = Scope {
        repo_root,
        work_dir,
        key,
    };
    let reasons = evaluate(&scope, &entry.accepted, tip, required);
    Verdict::Evaluated { required, reasons }
}

/// The state after an evaluation of `tip` and the entry to record: the latch
/// takes `required`, so it rises with the mode and never falls here.
fn settle(
    key: &str,
    entry: &TargetEntry,
    tip: &str,
    required: bool,
    reasons: Vec<HoldReason>,
) -> (GuardState, TargetEntry) {
    if reasons.is_empty() {
        let (from, to) = (abbrev(&entry.accepted), abbrev(tip));
        tracing::info!("target guard: {key} advanced outside loom {from}..{to}; accepted");
        let updated = TargetEntry {
            accepted: tip.to_string(),
            hold: None,
            attestation: required,
        };
        return (
            GuardState::Clear {
                accepted: tip.to_string(),
            },
            updated,
        );
    }
    let hold = new_hold(entry, tip, reasons);
    let updated = TargetEntry {
        accepted: entry.accepted.clone(),
        hold: Some(hold.clone()),
        attestation: required,
    };
    (GuardState::Held(hold), updated)
}

/// A hold of `tip`; it keeps the recorded hold's `since` when that hold was
/// for the same tip.
fn new_hold(entry: &TargetEntry, tip: &str, reasons: Vec<HoldReason>) -> Hold {
    let since = entry
        .hold
        .as_ref()
        .filter(|hold| hold.observed == tip)
        .map_or_else(Utc::now, |hold| hold.since);
    Hold {
        accepted: entry.accepted.clone(),
        observed: tip.to_string(),
        reasons,
        since,
    }
}

/// The hold of `tip` when the record cannot be read; nothing is written.
fn unreadable_hold(tip: &str, error: &anyhow::Error) -> Hold {
    Hold {
        accepted: String::new(),
        observed: tip.to_string(),
        reasons: vec![HoldReason::Unevaluable {
            error: one_line(error),
        }],
        since: Utc::now(),
    }
}

/// Record `tip` as `key`'s accepted tip, latched when attestation is on now,
/// and create the ledger.
fn trust_on_first_use(
    repo_root: &Path,
    work_dir: &Path,
    mut record: GuardRecord,
    key: &str,
    tip: &str,
) -> Result<GuardState> {
    let attestation = attestation_mode(repo_root, work_dir) == AttestationMode::Active;
    let entry = TargetEntry {
        accepted: tip.to_string(),
        hold: None,
        attestation,
    };
    record.targets.insert(key.to_string(), entry);
    create_ledger(work_dir)?;
    write_record(work_dir, &record)?;
    tracing::info!("target guard: recorded {key} at {}", abbrev(tip));
    Ok(GuardState::Clear {
        accepted: tip.to_string(),
    })
}

/// The hold `check` would record for `target`'s current tip, without writing
/// anything or taking the merge lock. `None` when the record has no entry for
/// `target` or the move would be accepted. A record that cannot be read gives
/// the unevaluable hold [`check_locked`] returns for it.
pub fn pending_hold(repo_root: &Path, work_dir: &Path, target: &str) -> Result<Option<Hold>> {
    let key = target_key(target);
    let record = match read_record(work_dir) {
        Ok(record) => record,
        Err(error) => return Ok(Some(unreadable_hold(&target_tip(repo_root, key)?, &error))),
    };
    let Some(entry) = record.targets.get(key) else {
        return Ok(None);
    };
    let tip = target_tip(repo_root, key)?;
    Ok(match judge(repo_root, work_dir, key, entry, &tip) {
        Verdict::Accepted => None,
        Verdict::Memo(hold) => Some(hold),
        Verdict::Evaluated { reasons, .. } if reasons.is_empty() => None,
        Verdict::Evaluated { reasons, .. } => Some(new_hold(entry, &tip, reasons)),
    })
}

/// Whether `commit` is in `target`'s accepted tip; without an entry, whether
/// it is in the live target.
pub fn merged_into_accepted(
    repo_root: &Path,
    work_dir: &Path,
    target: &str,
    commit: &str,
) -> Result<bool> {
    match accepted_tip(work_dir, target)? {
        Some(accepted) => is_ancestor_of(commit, &accepted, repo_root),
        None => verify_merge_succeeded(commit, target_key(target), repo_root),
    }
}

/// The operator's acceptance of `target`'s current tip, which `expected`
/// (any commit name, an abbreviated id too) must name. Clears the hold and
/// sets the latch to the current attestation mode: the only place it falls.
/// A record that does not parse is replaced by a fresh one; a record that
/// cannot be read is an error.
pub fn accept(repo_root: &Path, work_dir: &Path, target: &str, expected: &str) -> Result<Accepted> {
    let key = target_key(target);
    let lock = MergeLock::acquire(work_dir, ACCEPT_LOCK_TIMEOUT)
        .context("Could not acquire the merge lock")?;
    let tip = target_tip(repo_root, key)?;
    let wanted = rev_parse(repo_root, expected)
        .with_context(|| format!("{expected} does not name a commit"))?;
    if wanted != tip {
        bail!("the target moved since you reviewed it: {key} is now at {tip}; review again");
    }
    let mut record = read_record_for_accept(work_dir)?;
    let entry = TargetEntry {
        accepted: tip.clone(),
        hold: None,
        attestation: attestation_mode(repo_root, work_dir) == AttestationMode::Active,
    };
    let previous = record.targets.insert(key.to_string(), entry);
    if previous.is_none() {
        create_ledger(work_dir)?;
    }
    write_record(work_dir, &record)?;
    lock.release()?;
    tracing::info!("target guard: {key} accepted at {}", abbrev(&tip));
    let from = previous.map_or_else(|| tip.clone(), |entry| entry.accepted);
    Ok(Accepted { from, to: tip })
}

#[cfg(test)]
mod attestation_tests;
#[cfg(test)]
mod evaluate_tests;
#[cfg(test)]
mod memo_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;
