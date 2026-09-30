//! Human review response for a stage
//!
//! Allows a human to respond to a stage flagged for review via dispute-criteria.
//! Supports three actions: approve (queue a fresh session), force-complete (skip acceptance), reject (block).

use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::git::worktree::find_repo_root_from_cwd;
use crate::models::stage::{Stage, StageStatus};
use crate::orchestrator::adjudication::close_open_disputes;
use crate::orchestrator::coherence::live_worker_sessions;
use crate::verify::transitions::{load_stage, update_stage};

/// Handle human review response for a stage.
///
/// One of `approve`, `force_complete`, or `reject_reason` must be provided.
/// If none are provided, shows the current review status and available actions.
pub fn human_review(
    stage_id: String,
    approve: bool,
    force_complete: bool,
    reject_reason: Option<String>,
) -> Result<()> {
    let work_dir_buf = crate::commands::common::work_dir_path()?;
    let work_dir: &Path = &work_dir_buf;

    let stage = load_stage(&stage_id, work_dir)?;

    // If no action flag is provided, show current status
    if !approve && !force_complete && reject_reason.is_none() {
        return show_review_status(&stage_id, &stage);
    }

    // Verify the stage is in NeedsHumanReview
    if stage.status != StageStatus::NeedsHumanReview {
        bail!(
            "Stage '{}' is in '{}' state. human-review requires NeedsHumanReview.",
            stage_id,
            stage.status
        );
    }

    if approve {
        handle_approve(&stage_id, work_dir)
    } else if force_complete {
        handle_force_complete(&stage_id, work_dir)
    } else if let Some(reason) = reject_reason {
        handle_reject(&stage_id, &reason, work_dir)
    } else {
        unreachable!()
    }
}

/// Show current review status and available actions.
fn show_review_status(stage_id: &str, stage: &Stage) -> Result<()> {
    if stage.status != StageStatus::NeedsHumanReview {
        bail!(
            "Stage '{}' is in '{}' state, not awaiting human review.",
            stage_id,
            stage.status
        );
    }

    println!("Stage '{stage_id}' is awaiting human review.");
    println!();
    if let Some(ref reason) = stage.review_reason {
        println!("Review reason: {reason}");
    } else {
        println!("Review reason: (none recorded)");
    }
    println!();
    println!("Available actions:");
    println!("  loom stage human-review {stage_id} --approve         Queue a fresh session with fresh fix attempts");
    println!("  loom stage human-review {stage_id} --force-complete  Skip acceptance and mark as completed");
    println!(
        "  loom stage human-review {stage_id} --reject <reason> Block the stage with a reason"
    );

    Ok(())
}

/// Approve the review: queue a fresh session with fresh fix attempts.
///
/// A contract stage reaches NeedsHumanReview by exhausting its contract
/// respawn budget; left spent, the first contract writer the requeued
/// session spawns would end without freezing and re-escalate immediately.
/// Reset inside the same locked `update_stage` closure that performs the
/// requeue, after `try_approve_review` re-validates the on-disk status and
/// before the closure returns `Ok`, matching `skip_retry::persist_retry_delta`'s
/// ordering — so a refused transition (the on-disk stage no longer in
/// NeedsHumanReview) never resets the budget, and a failed reset aborts the
/// transition instead of leaving it approved with a still-spent budget.
fn handle_approve(stage_id: &str, work_dir: &Path) -> Result<()> {
    refuse_live_worker(stage_id, work_dir)?;
    // Disputes left open when the stage escalated would shadow the one the
    // fresh session files. Closing here also repairs a crash between an
    // escalation's status write and its own close. Before the transition, so
    // a crash after it cannot leave a queued stage with stale disputes.
    if load_stage(stage_id, work_dir)?.status == StageStatus::NeedsHumanReview {
        close_open_disputes(work_dir, stage_id);
    }
    update_stage(stage_id, work_dir, |stage| {
        stage.try_approve_review()?;
        stage.fix_attempts = 0;
        super::skip_retry::reset_contract_budget(stage, work_dir)?;
        Ok(())
    })?;

    println!("Stage '{stage_id}' approved: queued for a fresh session with fresh fix attempts.");

    Ok(())
}

/// Refuse to approve while the stage has a live worker session: the executor
/// would adopt it, by the same predicate, instead of spawning a fresh session.
fn refuse_live_worker(stage_id: &str, work_dir: &Path) -> Result<()> {
    let stage = load_stage(stage_id, work_dir)?;
    let live = live_worker_sessions(work_dir, &stage)?;
    if live.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = live.iter().map(|session| session.id.as_str()).collect();
    bail!(
        "Stage '{stage_id}' still has live worker session(s) {}; approving would queue the \
         stage onto that agent instead of a fresh session. Run 'loom stage reset {stage_id} \
         --kill-session' to take it down (this also resets the stage), or approve once it \
         has exited.",
        ids.join(", ")
    )
}

/// Force-complete the review: skip acceptance criteria and merge, then mark as completed.
///
/// Merge is attempted BEFORE transitioning to Completed so that if a conflict
/// occurs, the stage can move to MergeConflict/MergeBlocked via the valid
/// Executing→MergeConflict/MergeBlocked edges instead of the illegal
/// Completed→MergeConflict path.
fn handle_force_complete(stage_id: &str, work_dir: &Path) -> Result<()> {
    eprintln!(
        "WARNING: Force-completing stage '{stage_id}' without acceptance criteria verification."
    );

    // Transition to Executing directly (not via try_approve_review, which now
    // targets Queued) so all merge-outcome transitions are legal:
    //   Executing → MergeConflict | MergeBlocked | Completed
    // complete_with_merge handles Completed via try_complete(None) internally.
    let mut stage = update_stage(stage_id, work_dir, |stage| {
        stage.try_transition(StageStatus::Executing)?;
        stage.review_reason = None;
        Ok(())
    })?;

    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo_root = find_repo_root_from_cwd(&cwd).unwrap_or_else(|| cwd.clone());

    // Attempt progressive merge + completion. On Success, complete_with_merge
    // transitions Executing → Completed and triggers dependents. On
    // Conflict/Blocked it transitions to the appropriate merge state and saves.
    // Conflict and blocked outcomes are persisted by `complete_with_merge`, but
    // remain command failures so callers and automation cannot mistake them for
    // a successful force-completion.
    super::progressive_complete::complete_with_merge(&mut stage, &repo_root, work_dir)?;
    println!("Stage '{stage_id}' force-completed and merged successfully.");

    Ok(())
}

/// Reject the review: block the stage with a reason.
fn handle_reject(stage_id: &str, reason: &str, work_dir: &Path) -> Result<()> {
    update_stage(stage_id, work_dir, |stage| {
        stage.try_reject_review(reason.to_string())?;
        stage.close_reason = Some(reason.to_string());
        Ok(())
    })?;

    println!("Stage '{stage_id}' rejected and blocked.");
    println!("Reason: {reason}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::session_files::save_session;
    use crate::fs::work_dir::write_terminal_config;
    use crate::models::dispute::dispute_dir;
    use crate::models::session::{Session, SessionBackendKind, SessionStatus, TerminalConfig};
    use crate::orchestrator::terminal::native::write_test_pid_identity;
    use crate::verify::contracts::store::{attempts_spent, spend_attempt};
    use crate::verify::contracts::test_support::contract_stage;
    use tempfile::TempDir;

    fn setup_stage(temp: &TempDir, status: StageStatus, review_reason: Option<&str>) -> Stage {
        let stages_dir = temp.path().join("stages");
        std::fs::create_dir_all(&stages_dir).unwrap();

        let stage = Stage {
            id: "test-stage".to_string(),
            name: "Test Stage".to_string(),
            status,
            review_reason: review_reason.map(|s| s.to_string()),
            fix_attempts: 5,
            ..Default::default()
        };

        crate::verify::transitions::save_stage(&stage, temp.path()).unwrap();
        stage
    }

    #[test]
    fn test_human_review_approve() {
        let temp = TempDir::new().unwrap();
        setup_stage(&temp, StageStatus::NeedsHumanReview, Some("Bad criteria"));

        let work_dir = temp.path();
        let mut stage = load_stage("test-stage", work_dir).unwrap();
        assert_eq!(stage.status, StageStatus::NeedsHumanReview);
        assert_eq!(stage.fix_attempts, 5);

        stage.try_approve_review().unwrap();
        stage.fix_attempts = 0;

        assert_eq!(stage.status, StageStatus::Queued);
        assert_eq!(stage.fix_attempts, 0);
        assert_eq!(stage.review_reason, None);
    }

    #[test]
    fn test_human_review_approve_resets_an_unfrozen_stage_contract_budget() {
        let temp = TempDir::new().unwrap();
        let mut stage = contract_stage("test-stage", "writer-1");
        stage.status = StageStatus::NeedsHumanReview;
        crate::verify::transitions::save_stage(&stage, temp.path()).unwrap();
        for _ in 0..3 {
            spend_attempt(temp.path(), "test-stage").unwrap();
        }
        assert_eq!(attempts_spent(temp.path(), "test-stage").unwrap(), 3);

        handle_approve("test-stage", temp.path()).unwrap();

        assert_eq!(
            load_stage("test-stage", temp.path()).unwrap().status,
            StageStatus::Queued
        );
        assert_eq!(attempts_spent(temp.path(), "test-stage").unwrap(), 0);
    }

    /// A refused transition (on-disk status no longer `NeedsHumanReview` by
    /// the time the locked closure re-reads it) must not reset the budget:
    /// the reset lives inside the same closure, after `try_approve_review`
    /// re-validates, so its error path is never reached.
    #[test]
    fn test_human_review_approve_refused_transition_leaves_budget_unspent() {
        let temp = TempDir::new().unwrap();
        let mut stage = contract_stage("test-stage", "writer-1");
        // On-disk status is Completed, not NeedsHumanReview: `try_approve_review`'s
        // inner `NeedsHumanReview -> Queued` transition refuses it (Completed is
        // terminal, mirroring `test_human_review_wrong_state`).
        stage.status = StageStatus::Completed;
        crate::verify::transitions::save_stage(&stage, temp.path()).unwrap();
        for _ in 0..3 {
            spend_attempt(temp.path(), "test-stage").unwrap();
        }
        assert_eq!(attempts_spent(temp.path(), "test-stage").unwrap(), 3);

        let result = handle_approve("test-stage", temp.path());

        assert!(result.is_err());
        assert_eq!(
            load_stage("test-stage", temp.path()).unwrap().status,
            StageStatus::Completed
        );
        assert_eq!(attempts_spent(temp.path(), "test-stage").unwrap(), 3);
    }

    /// Approval settles the disputes the escalation left open, so the one a
    /// fresh session files next is the only live dispute of the stage.
    #[test]
    fn test_human_review_approve_closes_an_open_dispute() {
        let temp = TempDir::new().unwrap();
        setup_stage(&temp, StageStatus::NeedsHumanReview, Some("Escalated"));
        let dispute = dispute_dir(&temp.path().join("disputes"), "test-stage", 1);
        std::fs::create_dir_all(&dispute).unwrap();

        handle_approve("test-stage", temp.path()).unwrap();

        assert!(dispute.join("closed.marker").exists());
    }

    /// A stage not awaiting review keeps its disputes open: a refused approve
    /// must not settle anything.
    #[test]
    fn test_human_review_refused_approve_leaves_disputes_open() {
        let temp = TempDir::new().unwrap();
        setup_stage(&temp, StageStatus::Completed, None);
        let dispute = dispute_dir(&temp.path().join("disputes"), "test-stage", 1);
        std::fs::create_dir_all(&dispute).unwrap();

        assert!(handle_approve("test-stage", temp.path()).is_err());

        assert!(!dispute.join("closed.marker").exists());
    }

    /// Approval queues the stage, and the executor adopts a live worker
    /// session instead of spawning a fresh one, so approval refuses while one
    /// is live. The test process stands in for the agent: alive throughout,
    /// and nothing is left running after the test.
    #[test]
    fn test_human_review_approve_refuses_while_a_worker_session_is_live() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path();
        setup_stage(&temp, StageStatus::NeedsHumanReview, Some("Agent live"));
        let tmux = TerminalConfig {
            backend: SessionBackendKind::Tmux,
        };
        write_terminal_config(work_dir, &tmux).unwrap();
        let mut worker = Session::new();
        worker.assign_to_stage("test-stage".to_string());
        worker.status = SessionStatus::Running;
        worker.backend = SessionBackendKind::Tmux;
        save_session(&worker, work_dir).unwrap();
        write_test_pid_identity(work_dir, &worker, std::process::id()).unwrap();

        let message = format!("{:#}", handle_approve("test-stage", work_dir).unwrap_err());
        assert!(message.contains(&worker.id), "{message}");
        assert!(
            message.contains("loom stage reset test-stage --kill-session"),
            "{message}"
        );
        let refused = load_stage("test-stage", work_dir).unwrap();
        assert_eq!(refused.status, StageStatus::NeedsHumanReview);

        // Gone, as a takedown records it.
        worker.status = SessionStatus::ContextExhausted;
        save_session(&worker, work_dir).unwrap();
        handle_approve("test-stage", work_dir).unwrap();
        let approved = load_stage("test-stage", work_dir).unwrap();
        assert_eq!(approved.status, StageStatus::Queued);
    }

    #[test]
    fn test_human_review_reject() {
        let temp = TempDir::new().unwrap();
        setup_stage(&temp, StageStatus::NeedsHumanReview, Some("Bad criteria"));

        let work_dir = temp.path();
        let mut stage = load_stage("test-stage", work_dir).unwrap();
        assert_eq!(stage.status, StageStatus::NeedsHumanReview);

        stage
            .try_reject_review("Not needed anymore".to_string())
            .unwrap();

        assert_eq!(stage.status, StageStatus::Blocked);
        assert_eq!(stage.review_reason, Some("Not needed anymore".to_string()));
    }

    #[test]
    fn test_human_review_wrong_state() {
        // Completed is terminal: try_approve_review's inner NeedsHumanReview
        // -> Queued transition must refuse it regardless of the
        // command-level NeedsHumanReview check tested elsewhere.
        let mut stage = Stage {
            status: StageStatus::Completed,
            ..Default::default()
        };
        let result = stage.try_approve_review();
        assert!(result.is_err());
    }
}
