//! Git merge operations for integrating worktree branches

mod checkout_apply;
mod checkout_files;
mod checkout_state;
pub mod control_paths;
mod fast_forward;
pub mod in_progress;
mod inputs;
pub mod lock;
mod operation;
mod resolved;
mod status;
mod tree;

use anyhow::{bail, Result};
use std::path::Path;
use std::time::Duration;

use super::branch::{branch_exists, branch_name_for_stage, is_ancestor_of};
use crate::git::runner::run_git_checked;
use lock::MergeLock;
use operation::operator_operation;

// Re-export status types for use by other modules
pub use control_paths::{control_path_violation, control_path_violation_as, MergeGate};
pub use in_progress::{
    detect_in_progress_merge_at, detect_in_progress_merge_at_worktree, detect_in_progress_merges,
    git_dir_for_repo_path, merge_head_exists, ActiveMergeState, InProgressMerge, MergeLocation,
};
pub use inputs::blocked_merge_inputs;
pub use resolved::check_resolved_worktree;
pub use status::{build_merge_report, check_merge_state, MergeState, MergeStatusReport};
pub use tree::{
    advance_target, commit_merge, merge_tree, Advance, MergeBlock, PendingMerge, StashReapply,
    TreeMerge,
};

/// Result of a merge operation
#[derive(Debug, Clone)]
pub enum MergeResult {
    /// The target branch now points at a two-parent merge commit.
    Success {
        /// Number of files changed
        files_changed: u32,
        /// Number of insertions
        insertions: u32,
        /// Number of deletions
        deletions: u32,
        /// The operator's stashed changes, when they were stashed around
        /// the merge.
        stash: Option<StashReapply>,
    },
    /// Merge has conflicts that need resolution; nothing was changed.
    Conflict {
        /// List of files with conflicts
        conflicting_files: Vec<String>,
    },
    /// Nothing to merge (the stage branch is already in the target)
    AlreadyUpToDate,
    /// The merge was computed but the target was not advanced.
    Blocked(MergeBlock),
    /// The control-path gate refused the branch ([`MergeGate::Enforce`]);
    /// nothing was changed. `reason` is the human-review reason.
    Held { reason: String },
}

/// Merge a stage branch into the target branch (typically main) without
/// changing the main checkout beyond a final fast-forward.
///
/// Steps, under the merge lock:
/// 1. Refuse while the operator has a merge, cherry-pick, revert or rebase
///    in progress in the checkout.
/// 2. With [`MergeGate::Enforce`], hold a branch whose diff touches a control
///    path. The gate reads the same two commits that are merged, under the
///    lock, so a commit made to the branch while the lock was awaited cannot
///    slip past it.
/// 3. Compute the merge with `git merge-tree`; conflicts leave everything
///    untouched.
/// 4. Advance the target with [`advance_target`], which writes the merge
///    commit with `git commit-tree` only when the advance can happen.
pub fn merge_stage(
    stage_id: &str,
    target_branch: &str,
    repo_root: &Path,
    work_dir: &Path,
    gate: MergeGate,
) -> Result<MergeResult> {
    let _lock = MergeLock::acquire(work_dir, Duration::from_secs(30)).map_err(|e| {
        anyhow::anyhow!(
            "Could not acquire merge lock: {}. Another merge may be in progress.",
            e
        )
    })?;

    if let Some(marker) = operator_operation(repo_root)? {
        return Ok(MergeResult::Blocked(MergeBlock::OperatorOperation {
            marker,
        }));
    }

    let branch_name = branch_name_for_stage(stage_id);
    if !branch_exists(&branch_name, repo_root)? {
        bail!("Branch '{branch_name}' does not exist");
    }
    // Full ref names: a tag named like the branch must not win.
    let old = tree::rev_parse(repo_root, &format!("refs/heads/{target_branch}"))?;
    let branch_tip = tree::rev_parse(repo_root, &format!("refs/heads/{branch_name}"))?;
    if is_ancestor_of(&branch_tip, &old, repo_root)? {
        return Ok(MergeResult::AlreadyUpToDate);
    }

    if gate == MergeGate::Enforce {
        // Fails closed: a diff that cannot be computed is an error, not a pass.
        let label = branch_name_for_stage(stage_id);
        if let Some(reason) = control_path_violation_as(repo_root, &old, &branch_tip, &label)? {
            return Ok(MergeResult::Held { reason });
        }
    }

    merge_and_advance(repo_root, stage_id, target_branch, &old, &branch_tip)
}

/// Compute and land the merge of `branch_tip` into `old`.
fn merge_and_advance(
    repo_root: &Path,
    stage_id: &str,
    target_branch: &str,
    old: &str,
    branch_tip: &str,
) -> Result<MergeResult> {
    let merged_tree = match merge_tree(repo_root, old, branch_tip)? {
        TreeMerge::Clean { tree } => tree,
        TreeMerge::Conflict { paths } => {
            return Ok(MergeResult::Conflict {
                conflicting_files: paths,
            })
        }
    };
    let msg = format!(
        "Merge {} into {target_branch}",
        branch_name_for_stage(stage_id)
    );
    let shortstat = run_git_checked(&["diff", "--shortstat", old, &merged_tree], repo_root)?;
    let (files_changed, insertions, deletions) = parse_merge_stats(&shortstat);
    let mut pending = PendingMerge::new(repo_root, &merged_tree, [old, branch_tip], &msg);

    Ok(
        match advance_target(repo_root, target_branch, stage_id, &mut pending)? {
            Advance::Advanced { stash } => MergeResult::Success {
                files_changed,
                insertions,
                deletions,
                stash,
            },
            Advance::Blocked(block) => MergeResult::Blocked(block),
        },
    )
}

/// Parse merge statistics from git output
fn parse_merge_stats(output: &str) -> (u32, u32, u32) {
    let mut files_changed = 0u32;
    let mut insertions = 0u32;
    let mut deletions = 0u32;

    for line in output.lines() {
        // Look for line like: "3 files changed, 10 insertions(+), 5 deletions(-)"
        if line.contains("files changed") || line.contains("file changed") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if (*part == "files" || *part == "file") && i > 0 {
                    files_changed = parts[i - 1].parse().unwrap_or(0);
                }
                if part.contains("insertion") && i > 0 {
                    insertions = parts[i - 1].parse().unwrap_or(0);
                }
                if part.contains("deletion") && i > 0 {
                    deletions = parts[i - 1].parse().unwrap_or(0);
                }
            }
        }
    }

    (files_changed, insertions, deletions)
}

/// Get list of files with conflicts (during an active merge)
pub fn get_conflicting_files(repo_root: &Path) -> Result<Vec<String>> {
    let stdout = run_git_checked(&["diff", "--name-only", "--diff-filter=U"], repo_root)?;
    let files: Vec<String> = stdout
        .lines()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    Ok(files)
}

/// Verify that a merge actually succeeded by checking git ancestry.
///
/// This prevents "phantom merges" where the merged flag is set but the code
/// was never actually integrated. We verify by checking if the stage's
/// completed commit is reachable from the target branch.
///
/// # Arguments
/// * `completed_commit` - The commit SHA from when the stage completed
/// * `target_branch` - The branch we should have merged into (e.g., "main")
/// * `repo_root` - Path to the git repository root
///
/// # Returns
/// * `Ok(true)` if the commit is in the target branch's history (merge verified)
/// * `Ok(false)` if the commit is NOT in the target branch's history (merge not verified)
/// * `Err` if git command fails
pub fn verify_merge_succeeded(
    completed_commit: &str,
    target_branch: &str,
    repo_root: &Path,
) -> Result<bool> {
    is_ancestor_of(completed_commit, target_branch, repo_root)
}

#[cfg(test)]
mod stage_overlap_tests;
#[cfg(test)]
mod stage_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
