//! `loom target status`: the guard's view of the target branch.

use anyhow::Result;
use std::path::Path;

use super::{abbrev, resolve_context};
use crate::git::branch::branch_ref;
use crate::git::merge::rev_parse;
use crate::git::target_guard::{
    accept_command, accepted_tip, attestation_latched, attestation_mode, pending_hold,
    recorded_hold, recorded_holds, restore_commands, target_key, AttestationMode, Hold,
    RECORD_FILE,
};

/// Print the report for the current workspace's target. Read-only: it takes
/// no lock and writes nothing, and exits 0 whatever it finds.
pub fn execute() -> Result<()> {
    let (work_dir, repo_root, target) = resolve_context()?;
    print!("{}", report(&repo_root, &work_dir, &target)?);
    Ok(())
}

/// The report text: target, accepted and current tips, attestation, and the
/// state of the target with the commands to review, accept or restore a hold.
pub(crate) fn report(repo_root: &Path, work_dir: &Path, target: &str) -> Result<String> {
    let key = target_key(target);
    let current = rev_parse(repo_root, &branch_ref(key))?;
    if let Err(error) = recorded_holds(work_dir) {
        return Ok(unreadable_report(work_dir, key, &current, &error));
    }
    let accepted = accepted_tip(work_dir, key)?;
    let mut lines = vec![
        format!("Target: {key}"),
        format!(
            "Accepted: {}",
            accepted
                .as_deref()
                .unwrap_or("not recorded yet (the daemon records it at start)")
        ),
        format!("Current: {current}"),
        attestation_line(repo_root, work_dir, key)?,
    ];
    match (recorded_hold(work_dir, key)?, accepted) {
        (Some(hold), _) => lines.extend(held_lines(repo_root, key, &hold)?),
        (None, Some(accepted)) if accepted == current => lines.push("State: in sync".into()),
        (None, Some(accepted)) => {
            let pending = pending_hold(repo_root, work_dir, key)?;
            lines.extend(moved_lines(&accepted, &current, pending));
        }
        (None, None) => lines.push("State: not guarded yet".into()),
    }
    Ok(lines.join("\n") + "\n")
}

/// The report when the guard record cannot be read: the daemon holds the
/// target as unevaluable with no accepted tip, so the only way out is to
/// accept the current tip.
fn unreadable_report(work_dir: &Path, key: &str, current: &str, error: &anyhow::Error) -> String {
    let lines = [
        format!("Target: {key}"),
        format!("Current: {current}"),
        format!(
            "Guard record {} is unreadable: {error:#}",
            work_dir.join(RECORD_FILE).display()
        ),
        "State: HELD as unevaluable; no accepted tip is known".to_string(),
        format!("Accept: loom target accept --to {current}"),
    ];
    lines.join("\n") + "\n"
}

fn attestation_line(repo_root: &Path, work_dir: &Path, key: &str) -> Result<String> {
    Ok(match attestation_mode(repo_root, work_dir) {
        AttestationMode::Active => "Attestation: on".to_string(),
        AttestationMode::Off { reason } if attestation_latched(work_dir, key)? => format!(
            "Attestation: off ({reason}); this run recorded it on, so every move without a \
             ledger line holds until loom target accept"
        ),
        AttestationMode::Off { reason } => format!("Attestation: off ({reason})"),
    })
}

/// The lines for a recorded hold: why, how to review it, and how to accept or
/// restore it.
fn held_lines(repo_root: &Path, key: &str, hold: &Hold) -> Result<Vec<String>> {
    let mut lines = vec![format!("State: HELD since {}", hold.since)];
    lines.extend(hold.reasons.iter().map(|reason| format!("  - {reason}")));
    lines.push(format!(
        "Review: git log --oneline {}..{}",
        hold.accepted, hold.observed
    ));
    lines.push(format!(
        "        git diff --stat {} {}",
        hold.accepted, hold.observed
    ));
    lines.push(format!("Accept: {}", accept_command(key, hold)));
    let restore = restore_commands(repo_root, key, hold)?;
    if !restore.is_empty() {
        lines.push("Restore:".into());
        lines.extend(restore.into_iter().map(|command| format!("  {command}")));
    }
    Ok(lines)
}

/// The lines for a target that moved with no hold recorded yet: what the next
/// evaluation would do.
fn moved_lines(accepted: &str, current: &str, pending: Option<Hold>) -> Vec<String> {
    let mut lines = vec![format!(
        "State: moved {}..{}, not yet evaluated",
        abbrev(accepted),
        abbrev(current)
    )];
    match pending {
        Some(hold) => {
            lines.push("  it would be held:".into());
            lines.extend(hold.reasons.iter().map(|reason| format!("  - {reason}")));
        }
        None => lines.push("  it would be accepted".into()),
    }
    lines
}
