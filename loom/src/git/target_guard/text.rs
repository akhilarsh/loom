//! Operator-facing text for a hold: the one-line alert, the accept command
//! and the commands that restore the accepted tip.

use anyhow::Result;
use std::fmt;
use std::path::Path;

use super::{target_key, Hold, HoldReason};
use crate::git::branch::branch_ref;
use crate::git::runner::run_git;

/// Paths a reason names before "and N more".
const SHOWN_PATHS: usize = 3;

impl fmt::Display for HoldReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFastForward => f.write_str("not a fast-forward"),
            Self::ControlPaths { paths } => write!(f, "touches {}", path_list(paths)),
            Self::Unattested { from, to, paths } => write!(
                f,
                "unattested move {}..{} touching {}",
                short(from),
                short(to),
                path_list(paths)
            ),
            Self::StageWork { branches } => write!(f, "carries work of {}", branches.join(", ")),
            Self::Unevaluable { error } => write!(f, "could not evaluate: {error}"),
        }
    }
}

/// An object id cut to 7 characters; "unknown" for the empty id of a hold
/// built from an unreadable record.
fn short(id: &str) -> &str {
    if id.is_empty() {
        "unknown"
    } else {
        id.get(..7).unwrap_or(id)
    }
}

/// At most [`SHOWN_PATHS`] paths, control characters escaped so a path
/// cannot break the line, then "and N more".
fn path_list(paths: &[String]) -> String {
    let shown: Vec<String> = paths
        .iter()
        .take(SHOWN_PATHS)
        .map(|path| path.escape_debug().to_string())
        .collect();
    match paths.len().saturating_sub(SHOWN_PATHS) {
        0 => shown.join(", "),
        rest => format!("{} and {rest} more", shown.join(", ")),
    }
}

/// One line for status views and logs: what moved, why it is held, and
/// where to review it.
pub fn hold_alert(target: &str, hold: &Hold) -> String {
    let target = target_key(target);
    let reasons: Vec<String> = hold.reasons.iter().map(ToString::to_string).collect();
    format!(
        "Target {target} held: moved outside loom {}→{} ({}). Merges into {target} wait. \
         Review: loom target status",
        short(&hold.accepted),
        short(&hold.observed),
        reasons.join("; ")
    )
}

/// The command that accepts the held tip of `target`; the CLI resolves the
/// target itself.
pub fn accept_command(_target: &str, hold: &Hold) -> String {
    format!("loom target accept --to {}", hold.observed)
}

/// The commands that put `target` back at the accepted tip: `update-ref`
/// guarded by the observed tip, plus a `read-tree` that moves the files of
/// the checkout at `repo_root` back when the target is checked out there.
/// None for a hold built from an unreadable record: it knows no accepted
/// tip.
pub fn restore_commands(repo_root: &Path, target: &str, hold: &Hold) -> Result<Vec<String>> {
    let (accepted, observed) = (&hold.accepted, &hold.observed);
    if accepted.is_empty() {
        return Ok(Vec::new());
    }
    let reference = branch_ref(target_key(target));
    let mut commands = vec![format!("git update-ref {reference} {accepted} {observed}")];
    let head = run_git(&["symbolic-ref", "-q", "HEAD"], repo_root)?;
    if head.status.success() && String::from_utf8_lossy(&head.stdout).trim() == reference {
        commands.push(format!(
            "git read-tree -m -u {observed} {accepted}  # run in {}",
            repo_root.display()
        ));
    }
    Ok(commands)
}
