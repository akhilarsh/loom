//! The tails of `loom stage merge`: record a block, or mark a landed merge
//! complete, trigger dependents and clean up.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::git::MergeBlock;
use crate::models::stage::Stage;
use crate::verify::transitions::{trigger_dependents, update_stage};

use super::finish::finish_merge_and_report;
use crate::commands::stage::progressive_complete::report_merge_block;

/// The main repository root, followed out of a worktree through the state
/// directory's symlink.
pub(super) fn main_repo_root(work_dir: &Path) -> Result<PathBuf> {
    crate::fs::work_dir::WorkDir::new(work_dir)?
        .main_project_root()
        .ok_or_else(|| {
            anyhow!("Could not resolve the main repository root from the state directory")
        })
}

/// Persist a typed merge block on the fresh on-disk stage and say what retries
/// it. The stage stays unmerged.
pub(super) fn record_block(stage_id: &str, work_dir: &Path, block: MergeBlock) -> Result<()> {
    let persisted = block.clone();
    update_stage(stage_id, work_dir, |s| {
        s.block_merge(persisted.clone());
        Ok(())
    })?;
    report_merge_block(stage_id, &block);
    Ok(())
}

/// Mark a resolved merge complete after ancestry proves it landed, then
/// trigger dependents and clean up. Cleanup defers when this runs inside the
/// stage worktree, which the resolver session still occupies.
pub(super) fn complete_resolved_merge(
    stage: &Stage,
    work_dir: &Path,
    repo_root: &Path,
    target_branch: &str,
) -> Result<()> {
    let stage_id = stage.id.as_str();
    // Without ancestry proof `--resolved` could be invoked after a partial
    // resolution and silently satisfy downstream dependency checks even though
    // the commit never landed. Refusal propagates.
    let verified = crate::commands::stage::merge_verify::verify_or_derive_completed_commit(
        stage,
        target_branch,
        repo_root,
    )?;
    let persist_commit = verified.persist_commit.clone();
    update_stage(stage_id, work_dir, |s| {
        if let Some(commit) = persist_commit.clone() {
            s.completed_commit = Some(commit);
        }
        s.clear_merge_block();
        s.try_complete_merge()
    })?;

    println!("Stage '{stage_id}' merge conflict resolution complete!");
    println!("  Status: Completed (merged: true)");
    trigger_and_report(stage_id, work_dir, repo_root, target_branch)?;
    finish_merge_and_report(stage_id, repo_root, work_dir, target_branch);
    Ok(())
}

/// Clear the conflict flag and merge block and mark the stage completed+merged
/// on the fresh on-disk stage (A-5), then trigger dependents and clean up. The
/// real merge already landed the commit in the target branch, so
/// `try_complete_merge`'s `merged = true` is a verified-merge success, not a
/// phantom merge.
pub(super) fn complete_retried_merge(
    stage_id: &str,
    work_dir: &Path,
    repo_root: &Path,
    target_branch: &str,
    done_message: &str,
) -> Result<()> {
    update_stage(stage_id, work_dir, |s| {
        s.merge_conflict = false;
        s.clear_merge_block();
        s.try_complete_merge()
    })?;
    println!();
    println!("{done_message}");
    trigger_and_report(stage_id, work_dir, repo_root, target_branch)?;
    finish_merge_and_report(stage_id, repo_root, work_dir, target_branch);
    Ok(())
}

/// Trigger dependent stages and print which ones started, if any. Shared by
/// every merge-completion path so the reporting stays identical across them.
pub(super) fn trigger_and_report(
    stage_id: &str,
    work_dir: &Path,
    repo_root: &Path,
    target_branch: &str,
) -> Result<()> {
    let triggered = trigger_dependents(stage_id, work_dir, repo_root, target_branch)
        .context("Failed to trigger dependent stages")?;
    if !triggered.is_empty() {
        println!("Triggered {} dependent stage(s):", triggered.len());
        for dep_id in &triggered {
            println!("  -> {dep_id}");
        }
    }
    Ok(())
}
