//! The guard's record ([`RECORD_FILE`]) and the refs file ([`REFS_FILE`])
//! the `reference-transaction` hook reads. Only loom on the host writes
//! either, under the merge lock.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use super::{target_key, Hold, RECORD_FILE, REFS_FILE};
use crate::fs::locking::{locked_read, locked_write};
use crate::git::branch::branch_ref;
use crate::sandbox::KNOWLEDGE_WRITE_GLOB;

/// The first line of [`REFS_FILE`].
const REFS_HEADER: &str = "# written by loom; read by .git/hooks/reference-transaction";

/// Every guarded target, keyed by short branch name.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuardRecord {
    pub(super) targets: BTreeMap<String, TargetEntry>,
}

/// One target's accepted tip, hold and attestation latch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetEntry {
    /// The last tip loom accepted.
    pub(super) accepted: String,
    /// The hold on a tip loom did not accept.
    pub(super) hold: Option<Hold>,
    /// The latch: while set, every evaluation walks the attestation ledger.
    pub(super) attestation: bool,
}

/// The path prefix knowledge stages write under (`doc/loom/knowledge/`).
pub(super) fn knowledge_prefix() -> &'static str {
    KNOWLEDGE_WRITE_GLOB
        .strip_suffix("**")
        .unwrap_or(KNOWLEDGE_WRITE_GLOB)
}

/// The contents of `path`, read under the state directory's lock; `None`
/// when it does not exist.
fn read_if_exists(path: &Path) -> Result<Option<String>> {
    let exists = path
        .try_exists()
        .with_context(|| format!("Failed to look for {}", path.display()))?;
    if !exists {
        return Ok(None);
    }
    locked_read(path).map(Some)
}

/// The record in `work_dir`; empty when the file does not exist. A file that
/// does not parse is an error.
pub(super) fn read_record(work_dir: &Path) -> Result<GuardRecord> {
    let path = work_dir.join(RECORD_FILE);
    let Some(text) = read_if_exists(&path)? else {
        return Ok(GuardRecord::default());
    };
    serde_json::from_str(&text).with_context(|| format!("{} does not parse", path.display()))
}

/// Write the refs file, then the record: a crash between the two leaves the
/// hook guarding a ref the record lacks, never the reverse.
pub(super) fn write_record(work_dir: &Path, record: &GuardRecord) -> Result<()> {
    locked_write(&work_dir.join(REFS_FILE), &refs_text(record))?;
    let json = serde_json::to_string_pretty(record)
        .context("Failed to serialize the target guard record")?;
    locked_write(&work_dir.join(RECORD_FILE), &json)
}

fn refs_text(record: &GuardRecord) -> String {
    let mut text = format!("{REFS_HEADER}\n");
    for key in record.targets.keys() {
        text.push_str(&format!("ref {}\n", branch_ref(key)));
    }
    text.push_str(&format!("allow {}\n", knowledge_prefix()));
    text
}

/// The refs the hook guards: the `ref` lines of [`REFS_FILE`], empty when
/// the file does not exist.
pub fn guarded_refs(work_dir: &Path) -> Result<Vec<String>> {
    let text = read_if_exists(&work_dir.join(REFS_FILE))?.unwrap_or_default();
    Ok(text
        .lines()
        .filter_map(|line| line.strip_prefix("ref "))
        .map(str::to_string)
        .collect())
}

/// The tip loom last accepted for `target`, if the record has an entry.
pub fn accepted_tip(work_dir: &Path, target: &str) -> Result<Option<String>> {
    let mut record = read_record(work_dir)?;
    Ok(record
        .targets
        .remove(target_key(target))
        .map(|entry| entry.accepted))
}

/// The hold recorded for `target`, if any.
pub fn recorded_hold(work_dir: &Path, target: &str) -> Result<Option<Hold>> {
    let mut record = read_record(work_dir)?;
    Ok(record
        .targets
        .remove(target_key(target))
        .and_then(|entry| entry.hold))
}

/// Every recorded hold with its target, in target order.
pub fn recorded_holds(work_dir: &Path) -> Result<Vec<(String, Hold)>> {
    Ok(read_record(work_dir)?
        .targets
        .into_iter()
        .filter_map(|(key, entry)| entry.hold.map(|hold| (key, hold)))
        .collect())
}

/// The record's latch for `target`: `true` once an evaluation found the mode
/// `Active`, until `accept` stores the then-current mode. `false` when the
/// entry is missing.
pub fn attestation_latched(work_dir: &Path, target: &str) -> Result<bool> {
    let record = read_record(work_dir)?;
    Ok(record
        .targets
        .get(target_key(target))
        .is_some_and(|entry| entry.attestation))
}

/// Loom's own advance of `target` from `from` to `to`: recorded, and any
/// hold cleared, when the accepted tip is `from`. Otherwise the record is
/// left as it is and the next evaluation judges the move. The caller holds
/// the merge lock.
pub fn record_advance(work_dir: &Path, target: &str, from: &str, to: &str) -> Result<()> {
    let mut record = read_record(work_dir)?;
    let Some(entry) = record.targets.get_mut(target_key(target)) else {
        return Ok(());
    };
    if entry.accepted != from {
        return Ok(());
    }
    entry.accepted = to.to_string();
    entry.hold = None;
    write_record(work_dir, &record)
}
