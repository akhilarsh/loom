//! Progressive merge integration for stage completion
//!
//! This module handles the git merge operations that occur when a stage
//! completes successfully with passing acceptance criteria.

use anyhow::{bail, Result};
use std::path::Path;

use crate::git::branch::branch_name_for_stage;
use crate::git::get_branch_head;
use crate::git::merge::{MergeBlock, MergeGate, StashReapply};
use crate::models::stage::Stage;
use crate::orchestrator::merge_lifecycle::MergeLifecycle;
use crate::orchestrator::{get_merge_point, merge_completed_stage, ProgressiveMergeResult};
use crate::verify::transitions::update_stage;

mod finish;

/// Result of attempting to merge a completed stage
pub enum MergeOutcome {
    /// Merge succeeded - stage can be marked completed
    Success,
    /// Merge conflict - stage should be marked MergeConflict
    Conflict,
    /// Merge blocked - stage should be marked MergeBlocked
    Blocked,
    /// The control-path gate held the branch - stage already routed to
    /// NeedsHumanReview
    Held,
}

/// Attempt to progressively merge a completed stage into the merge point.
///
/// This function handles the git merge operations and updates the stage's
/// merge-related fields (merged, completed_commit).
///
/// # Concurrency (A-5)
///
/// The in-memory `stage` handed to `complete_with_merge` was loaded before the
/// (potentially multi-minute) acceptance and verification phases ran, so it is
/// stale relative to concurrent daemon/dispute writes. The git merge itself runs
/// *outside* the stages-dir lock (it holds the separate `MergeLock` and can take
/// time), then the merge-completion fields are re-applied to the **fresh**
/// on-disk stage via `update_stage`. The completion operation owns exactly:
/// `completed_commit`, `merged`, `merge_conflict`, and the status transition; it
/// re-applies only those, so a concurrent writer's unrelated fields
/// (`dispute_count`, `retry_count`, amended `acceptance`, …) survive. The
/// in-memory `stage` is also updated so the caller can drive resolver spawning.
///
/// # Returns
/// - `MergeOutcome::Success` if merge succeeded (stage should be marked Completed)
/// - `MergeOutcome::Conflict` if merge conflict (stage already marked MergeConflict)
/// - `MergeOutcome::Blocked` if merge failed (stage already marked MergeBlocked)
/// - `MergeOutcome::Held` if `gate` is `Enforce` and the branch touches a control
///   path (stage already routed to NeedsHumanReview)
pub fn attempt_progressive_merge(
    stage: &mut Stage,
    repo_root: &Path,
    work_dir: &Path,
    gate: MergeGate,
) -> Result<MergeOutcome> {
    let merge_point = get_merge_point(work_dir)?;

    let completed_commit = capture_completed_commit(stage, repo_root);

    println!("Attempting progressive merge into '{merge_point}'...");
    match merge_completed_stage(stage, repo_root, &merge_point, gate) {
        Ok(ProgressiveMergeResult::Success {
            files_changed,
            stash,
        }) => {
            println!("  ✓ Merged {files_changed} file(s) into '{merge_point}'");
            report_stash(stash.as_ref());
            stage.merged = true;
            Ok(MergeOutcome::Success)
        }
        Ok(ProgressiveMergeResult::AlreadyMerged) => {
            println!("  ✓ Already up to date with '{merge_point}'");
            stage.merged = true;
            Ok(MergeOutcome::Success)
        }
        Ok(ProgressiveMergeResult::NoBranch) => {
            block_missing_branch(stage, work_dir, completed_commit)
        }
        Ok(ProgressiveMergeResult::Blocked(block)) => {
            hold_blocked_merge(stage, work_dir, completed_commit, block)
        }
        Ok(ProgressiveMergeResult::Held { reason }) => {
            hold_for_review(stage, work_dir, completed_commit, &reason)
        }
        Ok(ProgressiveMergeResult::Conflict { conflicting_files }) => {
            let at = ConflictTarget {
                merge_point: &merge_point,
                repo_root,
                work_dir,
            };
            record_conflict(stage, &conflicting_files, completed_commit, &at)
        }
        Err(e) => block_failed_merge(stage, work_dir, completed_commit, &e),
    }
}

/// Capture the completed commit SHA from the stage branch HEAD and return the
/// commit to re-apply onto the fresh on-disk stage. The stage is only updated
/// when `get_branch_head` succeeds: overwriting a previously persisted
/// `completed_commit` with `None` would lose the ancestry proof.
fn capture_completed_commit(stage: &mut Stage, repo_root: &Path) -> Option<String> {
    if let Ok(commit) = get_branch_head(&branch_name_for_stage(&stage.id), repo_root) {
        stage.completed_commit = Some(commit);
    }
    stage.completed_commit.clone()
}

/// The merge could not be attempted: block the stage and say how to retry.
fn block_failed_merge(
    stage: &mut Stage,
    work_dir: &Path,
    completed_commit: Option<String>,
    error: &anyhow::Error,
) -> Result<MergeOutcome> {
    eprintln!("Progressive merge failed: {error}");
    let outcome = mark_blocked(stage, work_dir, completed_commit)?;
    eprintln!("Stage '{}' marked as MergeBlocked", stage.id);
    eprintln!("  Fix the issue and run: loom stage retry {}", stage.id);
    Ok(outcome)
}

/// A missing stage branch cannot prove anything landed: block the merge
/// instead of recording it as merged.
fn block_missing_branch(
    stage: &mut Stage,
    work_dir: &Path,
    completed_commit: Option<String>,
) -> Result<MergeOutcome> {
    tracing::error!(
        stage_id = %stage.id,
        "Progressive merge: branch missing — cannot verify merge succeeded"
    );
    mark_blocked(stage, work_dir, completed_commit)
}

/// Re-apply only the merge-block transition and the captured commit onto the
/// fresh on-disk stage (A-5).
fn mark_blocked(
    stage: &mut Stage,
    work_dir: &Path,
    completed_commit: Option<String>,
) -> Result<MergeOutcome> {
    stage.try_mark_merge_blocked()?;
    update_stage(&stage.id, work_dir, |s| {
        s.completed_commit = completed_commit.clone();
        s.try_mark_merge_blocked()
    })?;
    Ok(MergeOutcome::Blocked)
}

/// Tell the operator where their uncommitted tracked changes are when the
/// merge stashed them. Changes that were not reapplied are a warning: the
/// merge did land.
pub(super) fn report_stash(stash: Option<&StashReapply>) {
    match stash {
        Some(stash) if stash.restored => println!("  {}", stash.notice()),
        Some(stash) => eprintln!("  WARNING: {}", stash.notice()),
        None => {}
    }
}

/// Print why the merge is blocked and who retries it.
pub(super) fn report_merge_block(stage_id: &str, block: &MergeBlock) {
    println!("  Merge blocked: {block}");
    println!(
        "  A running daemon retries the merge every tick; otherwise run `loom stage merge {stage_id}`"
    );
}

/// Print why the control-path gate held the merge and how an operator
/// releases it.
pub(super) fn report_merge_hold(stage_id: &str, reason: &str) {
    println!("  Merge held: {reason}");
    println!(
        "  In .worktrees/{stage_id} review the change, then run \
         `loom stage human-review {stage_id} --force-complete`"
    );
}

/// Route `stage_id` to `NeedsHumanReview` on the fresh on-disk stage and
/// report the hold. For the commands that hold a branch without the progressive
/// merge's captured commit.
pub(super) fn route_held_to_review(stage_id: &str, work_dir: &Path, reason: &str) -> Result<()> {
    update_stage(stage_id, work_dir, |s| {
        s.route_to_review(reason);
        Ok(())
    })?;
    report_merge_hold(stage_id, reason);
    Ok(())
}

/// Route the stage to `NeedsHumanReview` on the fresh on-disk stage (A-5)
/// together with the captured commit, and report the hold. Nothing was merged.
fn hold_for_review(
    stage: &mut Stage,
    work_dir: &Path,
    completed_commit: Option<String>,
    reason: &str,
) -> Result<MergeOutcome> {
    stage.route_to_review(reason);
    update_stage(&stage.id, work_dir, |s| {
        s.completed_commit = completed_commit.clone();
        s.route_to_review(reason);
        Ok(())
    })?;
    report_merge_hold(&stage.id, reason);
    Ok(MergeOutcome::Held)
}

/// Persist a typed merge block on the fresh on-disk stage (A-5) together with
/// the captured commit, and report it. The merge was not attempted past the
/// block, so the checkout is untouched.
fn hold_blocked_merge(
    stage: &mut Stage,
    work_dir: &Path,
    completed_commit: Option<String>,
    block: MergeBlock,
) -> Result<MergeOutcome> {
    stage.block_merge(block.clone());
    let persisted = block.clone();
    update_stage(&stage.id, work_dir, |s| {
        s.completed_commit = completed_commit.clone();
        s.block_merge(persisted.clone());
        Ok(())
    })?;
    report_merge_block(&stage.id, &block);
    Ok(MergeOutcome::Blocked)
}

/// Where a conflicting merge was attempted.
struct ConflictTarget<'a> {
    merge_point: &'a str,
    repo_root: &'a Path,
    work_dir: &'a Path,
}

/// Persist `MergeConflict` with the captured commit (A-5), report the
/// conflicting files, and start a merge resolver for the stage.
fn record_conflict(
    stage: &mut Stage,
    conflicting_files: &[String],
    completed_commit: Option<String>,
    target: &ConflictTarget<'_>,
) -> Result<MergeOutcome> {
    println!("  ✗ Merge conflict detected!");
    println!("    Conflicting files:");
    for file in conflicting_files {
        println!("      - {file}");
    }
    println!();
    println!("    Stage transitioning to MergeConflict status.");
    stage.try_mark_merge_conflict()?;
    // Re-apply only the merge-conflict transition + commit onto the fresh
    // on-disk stage (A-5).
    update_stage(&stage.id, target.work_dir, |s| {
        s.completed_commit = completed_commit.clone();
        s.clear_merge_block();
        s.try_mark_merge_conflict()
    })?;
    spawn_resolver_and_report(stage, conflicting_files, target);
    Ok(MergeOutcome::Conflict)
}

/// Try to auto-spawn a merge resolver session. `merge_stage` computes the
/// merge without touching the checkout, so the resolver starts from a clean
/// stage worktree.
fn spawn_resolver_and_report(
    stage: &Stage,
    conflicting_files: &[String],
    target: &ConflictTarget<'_>,
) {
    use super::merge_resolver::MergeResolverResult;
    match super::merge_resolver::spawn_merge_resolver(
        stage,
        conflicting_files,
        target.merge_point,
        target.repo_root,
        target.work_dir,
    ) {
        Ok(MergeResolverResult::DaemonManaged) => {
            println!("    Daemon is running - merge resolution will be handled automatically.");
        }
        Ok(MergeResolverResult::Spawned(id)) => {
            println!("    Spawned merge resolver session: {id}");
        }
        Ok(MergeResolverResult::AlreadyRunning { session_id }) => {
            println!("    Merge resolver session '{session_id}' is already running.");
        }
        Err(e) => {
            eprintln!("    Failed to spawn merge resolver: {e}");
            println!(
                "    Resolve conflicts manually and run: loom stage merge {} --resolved",
                stage.id
            );
        }
    }
}

/// Whether worktree cleanup for `stage_id` must be deferred rather than run
/// now. See `merge_lifecycle::should_defer_cleanup` for the real
/// implementation; production code now reaches it only via
/// `MergeLifecycle::cleanup`. This delegation stays test-only (`cfg(test)`)
/// so `tests/progressive_complete.rs`'s regression coverage for the
/// deferral gate keeps compiling without leaving a dead production symbol.
#[cfg(test)]
pub(super) fn should_defer_cleanup(cwd: &Path, repo_root: &Path, stage_id: &str) -> bool {
    crate::orchestrator::merge_lifecycle::should_defer_cleanup(cwd, repo_root, stage_id)
}

/// Complete a stage with merge, triggering dependents on success.
///
/// This is the standard completion path for stages after acceptance criteria pass.
/// It attempts progressive merge and marks the stage as completed. `gate` is
/// `Bypass` only for an operator's forced completion of a reviewed branch.
pub fn complete_with_merge(
    stage: &mut Stage,
    repo_root: &Path,
    work_dir: &Path,
    gate: MergeGate,
) -> Result<bool> {
    MergeLifecycle::new(&stage.id, repo_root, work_dir).reconcile_overlay();

    match attempt_progressive_merge(stage, repo_root, work_dir, gate)? {
        MergeOutcome::Success => finish::finish_completion(stage, repo_root, work_dir),
        MergeOutcome::Conflict => {
            bail!(
                "Merge conflict detected for stage '{}'.\n\
                 A resolution session has been spawned to handle the merge.\n\
                 Your work is committed on the stage branch -- this session should exit now.\n\
                 Do NOT attempt to resolve the merge conflict yourself.",
                stage.id
            );
        }
        MergeOutcome::Blocked => {
            bail!(
                "Merge blocked for stage '{}'.\n\
                 The stage has been marked MergeBlocked.\n\
                 This session should exit now. Fix the issue and run: loom stage retry {}",
                stage.id,
                stage.id
            );
        }
        MergeOutcome::Held => {
            bail!(
                "Merge held for stage '{}': the branch touches a control path.\n\
                 The stage has been routed to NeedsHumanReview.\n\
                 This session should exit now; a person releases it.",
                stage.id
            );
        }
    }
}
