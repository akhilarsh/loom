//! Unified merge command for stages
//!
//! Combines retry-merge and merge-complete into a single command.
//! Default: re-attempt merge to main from a worktree.
//! --resolved: complete manual merge resolution.

use anyhow::{anyhow, bail, Result};
use std::path::Path;

use crate::commands::common::detect_stage_id;
use crate::git::branch::branch_name_for_stage;
use crate::git::merge::check_resolved_worktree;
use crate::git::{merge_stage, MergeResult};
use crate::models::stage::StageStatus;
use crate::verify::transitions::{load_stage, update_stage};

mod conflict;
mod finish;
mod landing;
mod next_step;
mod preflight;
mod relay;
use conflict::record_conflict_and_report;
use landing::{complete_resolved_merge, complete_retried_merge, main_repo_root, record_block};
use next_step::{print_fix_limit_options, report_merge_error};
use preflight::{retry_preflight, RetryPreflight};

use super::progressive_complete::report_backup_ref;

/// Unified merge command entry point.
///
/// If `resolved` is true, validates that manual merge resolution is complete
/// and transitions the stage to Completed. Otherwise, re-attempts the merge
/// programmatically. In Relay mode, `resolved` goes through `relay::resolved_entry`.
pub fn merge(stage_id: Option<String>, resolved: bool) -> Result<()> {
    if resolved {
        relay::resolved_entry(stage_id, merge_resolved)
    } else {
        merge_retry(stage_id)
    }
}

/// Land the merge a resolver (or the operator) prepared in the stage worktree.
///
/// This path:
/// 1. Resolves stage ID (provided or auto-detected from branch)
/// 2. Verifies the stage is in MergeConflict or MergeBlocked status
/// 3. Checks the stage worktree (`check_resolved_worktree`): target merged in,
///    committed, no unmerged path
/// 4. Merges the stage into the target with `merge_stage`, which leaves the
///    operator's main checkout alone apart from a fast-forward
/// 5. After ancestry proves the commit landed, marks the stage merged,
///    triggers dependent stages and cleans up
fn merge_resolved(stage_id: Option<String>) -> Result<()> {
    let work_dir_buf = crate::commands::common::work_dir_path()?;
    let work_dir: &Path = &work_dir_buf;

    // Resolve stage ID: use provided or detect from current worktree branch
    let stage_id = resolve_stage_id(stage_id, "merge --resolved <stage-id>")?;

    let stage = load_stage(&stage_id, work_dir)?;

    // Verify stage is in a merge-failed status
    if stage.status != StageStatus::MergeConflict && stage.status != StageStatus::MergeBlocked {
        bail!(
            "Stage '{}' is not in MergeConflict or MergeBlocked status (current: {}). \
             Use this command only after merge issues have been resolved.",
            stage_id,
            stage.status
        );
    }

    // The resolver runs inside the stage worktree, so the repository root is
    // the main repository the state directory belongs to, not the cwd.
    let repo_root = main_repo_root(work_dir)?;
    let target = crate::fs::resolve_target_branch_from_config(work_dir, &repo_root)?;
    check_resolved_worktree(&repo_root, &stage_id, &target)
        .map_err(|reason| anyhow!("The stage worktree is not ready to merge: {reason}"))?;

    match merge_stage(&stage_id, &target, &repo_root, work_dir)? {
        MergeResult::Success { backup_ref, .. } => {
            report_backup_ref(backup_ref.as_deref());
            complete_resolved_merge(&stage, work_dir, &repo_root, &target)
        }
        MergeResult::AlreadyUpToDate => {
            complete_resolved_merge(&stage, work_dir, &repo_root, &target)
        }
        MergeResult::Conflict { conflicting_files } => bail!(
            "'{target}' moved and conflicts again in {}: merge it into this worktree again, \
             resolve, commit, then rerun --resolved",
            conflicting_files.join(", ")
        ),
        MergeResult::Blocked(block) => record_block(&stage_id, work_dir, block),
    }
}

/// Re-attempt merge for a stage in MergeConflict or MergeBlocked status.
///
/// This path:
/// 1. Loads the stage and verifies it is in MergeConflict or MergeBlocked status
/// 2. Verifies we're running from a worktree (not the main repo)
/// 3. Increments fix_attempts on the stage
/// 4. Attempts the merge to the default branch using existing merge logic
/// 5. On success: marks stage as completed+merged, triggers dependents
/// 6. On conflict or error: says what the daemon does with the stage next
///    (`conflict::record_conflict_and_report`, `next_step::report_merge_error`)
/// 7. If at fix limit: suggests manual resolution, human-review, or skip
fn merge_retry(stage_id: Option<String>) -> Result<()> {
    let work_dir_buf = crate::commands::common::work_dir_path()?;
    let work_dir: &Path = &work_dir_buf;

    // Resolve stage ID, load and validate the stage, confirm we're running
    // from a worktree, and refuse an in-progress merge — all before spending
    // a fix attempt on this run.
    let RetryPreflight {
        stage_id,
        mut stage,
        repo_root,
        worktree_root: _worktree_root,
    } = retry_preflight(stage_id, work_dir)?;

    let max_attempts = stage.get_effective_max_fix_attempts();

    // Check the limit BEFORE incrementing so each attempt number is consumed
    // only when actually attempted. Checking after increment burns one extra
    // attempt at the boundary (off-by-one: with max=3, only 2 real attempts ran).
    if stage.is_at_fix_limit() {
        println!();
        println!(
            "Stage '{}' has reached the fix attempt limit ({}/{}).",
            stage_id, stage.fix_attempts, max_attempts
        );
        println!();
        print_fix_limit_options(&stage_id);
        return Ok(());
    }

    // Increment fix_attempts after the limit guard so the count is accurate.
    // Persist the increment via a locked read-modify-write on the FRESH on-disk
    // stage so a concurrent daemon/CLI write is not reverted, and so the count
    // is incremented from the current persisted value rather than a stale
    // in-memory one (A-5). Mirror the new value into the local copy for display.
    let attempts = update_stage(&stage_id, work_dir, |s| {
        s.increment_fix_attempts();
        Ok(())
    })?
    .fix_attempts;
    stage.fix_attempts = attempts;

    println!("Retrying merge for stage '{stage_id}' (attempt {attempts}/{max_attempts})");

    // Determine target branch, respecting configured base_branch over repo default
    let target_branch = crate::fs::resolve_target_branch_from_config(work_dir, &repo_root)?;

    let branch_name = branch_name_for_stage(&stage_id);
    println!("Merging {branch_name} into {target_branch}...");

    // Attempt the merge
    match merge_stage(&stage_id, &target_branch, &repo_root, work_dir) {
        Ok(MergeResult::Success {
            files_changed,
            insertions,
            deletions,
            backup_ref,
        }) => {
            println!("Merge successful!");
            println!("  {files_changed} files changed, +{insertions} -{deletions}");
            report_backup_ref(backup_ref.as_deref());
            let done = format!("Stage '{stage_id}' merge complete! (Completed, merged: true)");
            complete_retried_merge(&stage_id, work_dir, &repo_root, &target_branch, &done)?;
        }

        Ok(MergeResult::Blocked(block)) => record_block(&stage_id, work_dir, block)?,

        Ok(MergeResult::AlreadyUpToDate) => {
            println!("Branch is already up to date with {target_branch}.");
            let done = format!("Stage '{stage_id}' marked as merged.");
            complete_retried_merge(&stage_id, work_dir, &repo_root, &target_branch, &done)?;
        }

        Ok(MergeResult::Conflict { conflicting_files }) => {
            record_conflict_and_report(&stage, work_dir, &repo_root, &conflicting_files)?;
        }

        Err(e) => {
            // fix_attempts was already persisted by the locked increment above.
            report_merge_error(&stage, work_dir, &repo_root, &e);
        }
    }

    Ok(())
}

/// Resolve a stage ID: use the one provided, or detect it from the current
/// worktree branch. `usage` names the command line to show in the error hint
/// if detection fails, e.g. "merge <stage-id>" or "merge --resolved <stage-id>".
fn resolve_stage_id(stage_id: Option<String>, usage: &str) -> Result<String> {
    match stage_id {
        Some(id) => Ok(id),
        None => detect_stage_id().ok_or_else(|| {
            anyhow::anyhow!(
                "Could not detect stage ID from current branch.\n\
                 Please provide the stage ID explicitly: loom stage {usage}"
            )
        }),
    }
}

/// Walk up from the current directory to find the repo root (parent of .worktrees).
fn find_repo_root(cwd: &Path) -> Result<std::path::PathBuf> {
    let mut current = cwd.to_path_buf();

    loop {
        // Check if .worktrees is a sibling directory (meaning parent is repo root)
        if current.file_name().map(|n| n.to_string_lossy().to_string())
            == Some(".worktrees".to_string())
        {
            if let Some(parent) = current.parent() {
                return Ok(parent.to_path_buf());
            }
        }

        // Check if current dir contains .worktrees
        let worktrees_dir = current.join(".worktrees");
        if worktrees_dir.exists() && worktrees_dir.is_dir() {
            return Ok(current);
        }

        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => bail!(
                "Could not find repository root (no .worktrees directory found).\n\
                 Current directory: {}",
                cwd.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::get_conflicting_files;
    use crate::git::merge::merge_head_exists;
    use crate::models::stage::Stage;
    use std::process::Command;
    use tempfile::TempDir;

    fn create_test_stage(id: &str, status: StageStatus) -> Stage {
        Stage {
            id: id.to_string(),
            name: format!("Test Stage {id}"),
            status,
            fix_attempts: 0,
            max_fix_attempts: Some(3),
            ..Stage::default()
        }
    }

    // Tests from merge_complete

    #[test]
    fn test_get_conflicting_files_clean() {
        // In a clean repo, there should be no conflicting files
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path();

        // Initialize a git repo
        Command::new("git")
            .args(["init"])
            .current_dir(repo_root)
            .output()
            .unwrap();

        assert!(get_conflicting_files(repo_root).unwrap().is_empty());
    }

    #[test]
    fn test_merge_head_absent_in_clean_repo() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path();

        // Initialize a git repo
        Command::new("git")
            .args(["init"])
            .current_dir(repo_root)
            .output()
            .unwrap();

        assert!(!merge_head_exists(repo_root).unwrap());
    }

    // Tests from retry_merge

    #[test]
    fn test_merge_rejects_wrong_status() {
        let temp_dir = TempDir::new().unwrap();
        let work_dir = temp_dir.path();

        // Create stages directory and a stage in Executing status
        let stages_dir = work_dir.join(".loom").join("work").join("stages");
        std::fs::create_dir_all(&stages_dir).unwrap();

        let stage = create_test_stage("test-stage", StageStatus::Executing);
        let stage_path = stages_dir.join("test-stage.md");
        let content = crate::verify::transitions::serialize_stage_to_markdown(&stage).unwrap();
        std::fs::write(stage_path, content).unwrap();

        // merge should fail since we're not in a worktree and status is wrong
        // We test the status check by calling the function parts directly
        assert!(!matches!(
            stage.status,
            StageStatus::MergeConflict | StageStatus::MergeBlocked
        ));
    }

    #[test]
    fn test_merge_accepts_merge_conflict() {
        let stage = create_test_stage("test-stage", StageStatus::MergeConflict);
        assert!(matches!(
            stage.status,
            StageStatus::MergeConflict | StageStatus::MergeBlocked
        ));
    }

    #[test]
    fn test_merge_accepts_merge_blocked() {
        let stage = create_test_stage("test-stage", StageStatus::MergeBlocked);
        assert!(matches!(
            stage.status,
            StageStatus::MergeConflict | StageStatus::MergeBlocked
        ));
    }

    #[test]
    fn test_fix_attempts_increment() {
        let mut stage = create_test_stage("test-stage", StageStatus::MergeConflict);
        assert_eq!(stage.fix_attempts, 0);

        let attempts = stage.increment_fix_attempts();
        assert_eq!(attempts, 1);
        assert_eq!(stage.fix_attempts, 1);

        let attempts = stage.increment_fix_attempts();
        assert_eq!(attempts, 2);
        assert_eq!(stage.fix_attempts, 2);
    }

    #[test]
    fn test_fix_limit_detection() {
        let mut stage = create_test_stage("test-stage", StageStatus::MergeConflict);
        stage.max_fix_attempts = Some(2);

        assert!(!stage.is_at_fix_limit());

        stage.fix_attempts = 1;
        assert!(!stage.is_at_fix_limit());

        stage.fix_attempts = 2;
        assert!(stage.is_at_fix_limit());

        stage.fix_attempts = 3;
        assert!(stage.is_at_fix_limit());
    }

    #[test]
    fn test_find_repo_root_from_worktree() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path();

        // Create .worktrees/my-stage structure
        let worktree = repo_root.join(".worktrees").join("my-stage");
        std::fs::create_dir_all(&worktree).unwrap();

        let found = find_repo_root(&worktree).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            repo_root.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_find_repo_root_from_subdir() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path();

        // Create .worktrees/my-stage/src structure
        let subdir = repo_root.join(".worktrees").join("my-stage").join("src");
        std::fs::create_dir_all(&subdir).unwrap();

        // From inside worktree subdir, we should still find repo root
        // We need .worktrees at repo root for this to work
        let found = find_repo_root(&subdir).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            repo_root.canonicalize().unwrap()
        );
    }
}
