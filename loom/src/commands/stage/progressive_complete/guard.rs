//! The zero-commit guard of the CLI merge paths: a stage branch with no
//! commit beyond the target merges as a no-op, so a "merged" result would
//! stand for work that was never committed. Such a stage goes to human review,
//! as the daemon's auto-merge does.

use anyhow::Result;
use std::path::Path;

use crate::git::branch::{branch_exists, branch_name_for_stage, commits_ahead_of};
use crate::git::merge::verify_merge_succeeded;
use crate::models::stage::Stage;
use crate::verify::transitions::update_stage;

/// Why `stage` must go to human review instead of merging into `target`:
/// its branch exists with zero commits beyond `target` and no recorded
/// `completed_commit` is already in `target`. `None` lets the merge proceed;
/// a missing branch or a failed probe is left to the merge attempt itself.
pub(in crate::commands::stage) fn zero_commit_review_reason(
    stage: &Stage,
    target: &str,
    repo_root: &Path,
) -> Option<String> {
    let branch = branch_name_for_stage(&stage.id);
    if !branch_exists(&branch, repo_root).unwrap_or(false) {
        return None;
    }
    if !matches!(commits_ahead_of(&branch, target, repo_root), Ok(0)) {
        return None;
    }
    let landed = stage
        .completed_commit
        .as_deref()
        .is_some_and(|commit| verify_merge_succeeded(commit, target, repo_root).unwrap_or(false));
    if landed {
        return None;
    }
    Some(Stage::zero_commit_reason(&stage.id, target))
}

/// Route `stage_id` to `NeedsHumanReview` on the fresh on-disk stage with
/// `reason` and print it. Nothing was merged.
pub(in crate::commands::stage) fn route_zero_commit_to_review(
    stage_id: &str,
    work_dir: &Path,
    reason: &str,
) -> Result<()> {
    update_stage(stage_id, work_dir, |s| {
        s.route_to_review(reason);
        Ok(())
    })?;
    println!("  Merge not attempted: {reason}");
    Ok(())
}
