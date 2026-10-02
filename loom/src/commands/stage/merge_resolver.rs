//! Merge conflict resolver spawning for CLI path
//!
//! When the daemon is not running, this module handles spawning
//! a merge resolution session directly from the CLI.

use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::daemon::DaemonServer;
use crate::git::branch::branch_name_for_stage;
use crate::git::merge::control_path_violation;
use crate::models::session::Session;
use crate::models::stage::{Stage, StageStatus};
use crate::orchestrator::continuation::save_session;
use crate::orchestrator::signals::{find_live_merge_session_for_stage, generate_merge_signal};
use crate::orchestrator::terminal::backend::{merge_resolver_worktree, SessionBackend};

use super::progressive_complete::route_held_to_review;

/// Result of attempting to spawn a merge resolver session.
pub enum MergeResolverResult {
    /// Daemon is running and will handle merge resolution automatically.
    DaemonManaged,
    /// A merge resolver session was spawned with the given session ID.
    Spawned(String),
    /// A live merge resolver session is already running for this stage.
    AlreadyRunning { session_id: String },
}

/// Spawn a merge conflict resolver session from the CLI.
///
/// When the daemon is not running, this spawns a session through the configured backend
/// to resolve merge conflicts. If the daemon IS running, it returns early
/// since the daemon handles merge resolution automatically.
///
/// # Arguments
/// * `stage` - The stage with merge conflicts (must be in MergeConflict or MergeBlocked status)
/// * `conflicting_files` - List of files with conflicts
/// * `merge_point` - The target branch to merge into
/// * `repo_root` - Path to the main repository root, where `.worktrees/` lives
/// * `work_dir` - Path to the state directory
pub fn spawn_merge_resolver(
    stage: &Stage,
    conflicting_files: &[String],
    merge_point: &str,
    repo_root: &Path,
    work_dir: &Path,
) -> Result<MergeResolverResult> {
    // Validate stage is in an appropriate status for merge resolution
    if !matches!(
        stage.status,
        StageStatus::MergeConflict | StageStatus::MergeBlocked
    ) {
        bail!(
            "Cannot spawn merge resolver for stage '{}' in status '{}' (expected MergeConflict or MergeBlocked)",
            stage.id,
            stage.status
        );
    }

    // If daemon is running, it handles merge resolution automatically
    if DaemonServer::is_running(work_dir) {
        return Ok(MergeResolverResult::DaemonManaged);
    }

    // Refuse to spawn a duplicate resolver if a live one already exists.
    // (Stale signals are cleaned up by the helper.)
    if let Some(session_id) = find_live_merge_session_for_stage(&stage.id, work_dir)? {
        return Ok(MergeResolverResult::AlreadyRunning { session_id });
    }

    refuse_control_path_branch(stage, merge_point, repo_root, work_dir)?;

    spawn_resolver_session(stage, conflicting_files, merge_point, repo_root, work_dir)
}

/// Refuse to hand a branch that touches a control path to a resolver session:
/// the stage goes to human review and the spawn fails. A diff that cannot be
/// computed fails the spawn too.
fn refuse_control_path_branch(
    stage: &Stage,
    merge_point: &str,
    repo_root: &Path,
    work_dir: &Path,
) -> Result<()> {
    let branch = branch_name_for_stage(&stage.id);
    let violation = control_path_violation(repo_root, merge_point, &branch).with_context(|| {
        format!(
            "Cannot spawn a merge resolver for stage '{}': the control-path check failed",
            stage.id
        )
    })?;
    let Some(reason) = violation else {
        return Ok(());
    };
    route_held_to_review(&stage.id, work_dir, &reason)?;
    bail!(
        "Not spawning a merge resolver for stage '{}': the branch touches a control path",
        stage.id
    )
}

/// Write the merge signal and spawn the resolver in the stage worktree.
fn spawn_resolver_session(
    stage: &Stage,
    conflicting_files: &[String],
    merge_point: &str,
    repo_root: &Path,
    work_dir: &Path,
) -> Result<MergeResolverResult> {
    let backend = SessionBackend::from_config(work_dir.to_path_buf())
        .context("Failed to construct session backend for merge resolver")?;

    // The resolver works in the stage worktree; without it the operator
    // recreates the worktree or resolves by hand. Checked before the signal
    // is written so a missing worktree leaves nothing behind.
    let worktree = merge_resolver_worktree(repo_root, &stage.id).with_context(|| {
        format!(
            "Cannot spawn a merge resolver for stage '{}': recreate its worktree or resolve the \
             conflict by hand",
            stage.id
        )
    })?;

    let source_branch = branch_name_for_stage(&stage.id);
    let session = Session::new_merge(source_branch.clone(), merge_point.to_string());
    let session_id = session.id.clone();

    let signal_path = generate_merge_signal(
        &session,
        stage,
        &source_branch,
        merge_point,
        conflicting_files,
        work_dir,
    )
    .context("Failed to generate merge signal")?;

    let spawned_session = backend
        .spawn_merge_session_in_worktree(stage, &worktree, session, &signal_path)
        .context("Failed to spawn merge resolver session")?;

    save_session(&spawned_session, work_dir).context("Failed to save merge resolver session")?;

    Ok(MergeResolverResult::Spawned(session_id))
}
