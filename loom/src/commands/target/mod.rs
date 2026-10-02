//! `loom target`: review a move of the target branch that loom did not make.
//!
//! `status` reports the guard's view of the target (read-only); `accept`
//! records the operator's review of the current tip. `refuse_unreviewed_move`
//! keeps `loom clean` and `loom init --clean` from deleting the record while a
//! move is unreviewed.

pub mod accept;
pub mod status;
#[cfg(test)]
pub(crate) mod tests;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::commands::common::{resolve_state_dir, resolve_work_dir};
use crate::fs::resolve_target_branch_from_config;
use crate::git::cleanup::branch_exists_strict;
use crate::git::target_guard::{
    guarded_refs, pending_hold, recorded_holds, target_key, Hold, RECORD_FILE,
};

/// The state directory, main repository root and target branch of the
/// workspace containing the current directory. A malformed config is an
/// error, never a default target.
fn resolve_context() -> Result<(PathBuf, PathBuf, String)> {
    let work_dir = resolve_work_dir()?;
    let repo_root = work_dir
        .main_project_root()
        .context("Could not resolve the main repository root from the state directory")?;
    let target = resolve_target_branch_from_config(work_dir.root(), &repo_root)?;
    Ok((work_dir.root().to_path_buf(), repo_root, target))
}

/// Refuse to delete loom's state while the target holds a move loom did not
/// accept: removing the record would make the next run trust that move.
///
/// Reads only the guard record, never `config.toml`, so a malformed config
/// cannot block the recovery path. A record that cannot be read refuses; no
/// record proceeds.
pub(crate) fn refuse_unreviewed_move(repo_root: &Path) -> Result<()> {
    let work_dir = resolve_state_dir(repo_root);
    let record = work_dir.join(RECORD_FILE);
    if !record.exists() {
        return Ok(());
    }
    let unreadable = || format!("cannot read the target guard record {}", record.display());
    if let Some((target, hold)) = recorded_holds(&work_dir).with_context(unreadable)?.first() {
        return refusal(target, hold);
    }
    for reference in guarded_refs(&work_dir).with_context(unreadable)? {
        let target = target_key(&reference);
        if !branch_exists_strict(target, repo_root)? {
            bail!(
                "the target branch {target} no longer exists, so loom cannot tell whether it \
                 held a move loom did not accept; review the repository, then remove {} to \
                 proceed",
                record.display()
            );
        }
        if let Some(hold) = pending_hold(repo_root, &work_dir, target)
            .with_context(|| format!("cannot evaluate the target branch {target}"))?
        {
            return refusal(target, &hold);
        }
    }
    Ok(())
}

/// An object id cut to 12 characters.
pub(super) fn abbrev(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

fn refusal(target: &str, hold: &Hold) -> Result<()> {
    let reasons: Vec<String> = hold.reasons.iter().map(ToString::to_string).collect();
    bail!(
        "the target branch {target} has a move loom did not accept ({}); review it with \
         'loom target status', then accept or restore it before deleting loom's state",
        reasons.join("; ")
    )
}
