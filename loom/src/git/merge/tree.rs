//! Merge computed without touching the operator's main checkout.
//!
//! A merge is computed with `git merge-tree`, committed with
//! `git commit-tree`, and the target branch is advanced by `update-ref` (the
//! target is checked out nowhere) or `merge --ff-only` (checked out in the
//! main checkout, see [`super::checkout_apply`]). Every command runs through
//! `run_git`, which disables hooks and fsmonitor.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::git::runner::{run_git, run_git_checked};
use crate::git::worktree::list_worktrees;

/// Outcome of `git merge-tree --write-tree` for two commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeMerge {
    /// The merge is clean; `tree` is the merged tree object.
    Clean { tree: String },
    /// The merge conflicts in these paths (deduplicated, in git's order).
    Conflict { paths: Vec<String> },
}

/// Why the target branch was not advanced. Nothing in the main checkout was
/// changed when one of these is returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MergeBlock {
    /// The main checkout has an operator operation in progress.
    OperatorOperation { marker: String },
    /// The target branch is checked out in another worktree.
    TargetCheckedOutElsewhere { path: PathBuf },
    /// The target branch moved since the merge was computed.
    TargetMoved,
    /// Uncommitted work in the main checkout overlaps paths the merge touches.
    UncommittedOverlap { paths: Vec<String> },
    /// The merge landed but the local changes could not be restored; they
    /// are saved under `backup_ref`.
    ReapplyFailed { backup_ref: String },
}

impl fmt::Display for MergeBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperatorOperation { marker } => write!(
                f,
                "the main checkout has an operation in progress ({marker}); finish or abort it first"
            ),
            Self::TargetCheckedOutElsewhere { path } => write!(
                f,
                "the target branch is checked out in another worktree ({}); switch that worktree to another branch",
                path.display()
            ),
            Self::TargetMoved => write!(
                f,
                "the target branch moved while the merge was being prepared; it will be retried"
            ),
            Self::UncommittedOverlap { paths } => write!(
                f,
                "uncommitted changes in the main checkout overlap the merge ({}); commit, stash or move them",
                paths.join(", ")
            ),
            Self::ReapplyFailed { backup_ref } => write!(
                f,
                "the merge landed but local changes could not be restored; they are saved in {backup_ref} and in the stash list"
            ),
        }
    }
}

/// Result of [`advance_target`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advance {
    /// The target now points at the merge commit. `backup_ref` is set when
    /// local changes were stashed and reapplied around the fast-forward.
    Advanced { backup_ref: Option<String> },
    /// The target was not advanced.
    Blocked(MergeBlock),
}

/// Compute the merge of `branch` into `target` (any revisions) without
/// touching a working tree or a ref.
pub fn merge_tree(repo: &Path, target: &str, branch: &str) -> Result<TreeMerge> {
    let args = [
        "merge-tree",
        "--write-tree",
        "--name-only",
        "--no-messages",
        "-z",
        target,
        branch,
    ];
    let output = run_git(&args, repo)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fields = stdout.split('\0').filter(|f| !f.is_empty());
    let code = output.status.code();
    // Exit 0 and 1 both print the tree first; a bad revision exits 1 with
    // nothing on stdout and is an error like any other exit code.
    let tree = match (code, fields.next()) {
        (Some(0 | 1), Some(tree)) => tree.to_string(),
        _ => bail!(
            "git merge-tree {target} {branch} failed (exit code {}): {}",
            code.map_or_else(|| "signal".to_string(), |c| c.to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    };
    if code == Some(0) {
        return Ok(TreeMerge::Clean { tree });
    }
    let mut paths: Vec<String> = Vec::new();
    for path in fields {
        if !paths.iter().any(|p| p == path) {
            paths.push(path.to_string());
        }
    }
    Ok(TreeMerge::Conflict { paths })
}

/// Write a merge commit for `tree` with the given parents; returns its id.
pub fn commit_merge(repo: &Path, tree: &str, parents: [&str; 2], message: &str) -> Result<String> {
    run_git_checked(
        &[
            "commit-tree",
            tree,
            "-p",
            parents[0],
            "-p",
            parents[1],
            "-m",
            message,
        ],
        repo,
    )
}

/// Move `target` from `old` to `new` (a commit whose first parent is `old`).
///
/// `repo` is the operator's main checkout. Where the target is checked out
/// decides the mechanism: nowhere, `update-ref` guarded by `old`; in `repo`,
/// a fast-forward that keeps the operator's uncommitted work; in another
/// worktree, a block.
pub fn advance_target(
    repo: &Path,
    target: &str,
    old: &str,
    new: &str,
    stage_id: &str,
) -> Result<Advance> {
    match checked_out_at(repo, target)? {
        Some(path) if same_path(&path, repo) => {
            super::checkout_apply::advance_in_checkout(repo, old, new, stage_id)
        }
        Some(path) => Ok(Advance::Blocked(MergeBlock::TargetCheckedOutElsewhere {
            path,
        })),
        None => update_ref(repo, target, old, new, stage_id),
    }
}

/// Path of the worktree that has `branch` checked out, if any.
fn checked_out_at(repo: &Path, branch: &str) -> Result<Option<PathBuf>> {
    Ok(list_worktrees(repo)?
        .into_iter()
        .find(|wt| wt.branch.as_deref() == Some(branch))
        .map(|wt| wt.path))
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn update_ref(repo: &Path, target: &str, old: &str, new: &str, stage_id: &str) -> Result<Advance> {
    let reference = format!("refs/heads/{target}");
    let reason = format!("loom: merge loom/{stage_id}");
    let output = run_git(&["update-ref", "-m", &reason, &reference, new, old], repo)?;
    if output.status.success() {
        return Ok(Advance::Advanced { backup_ref: None });
    }
    if rev_parse(repo, &reference)? != old {
        return Ok(Advance::Blocked(MergeBlock::TargetMoved));
    }
    bail!(
        "git update-ref {reference} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

/// Resolve `rev` to a commit id.
pub(super) fn rev_parse(repo: &Path, rev: &str) -> Result<String> {
    run_git_checked(
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
        repo,
    )
}

#[cfg(test)]
mod tests;
