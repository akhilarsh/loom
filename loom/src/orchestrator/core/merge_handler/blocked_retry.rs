//! The per-tick retry of a merge held by a typed [`MergeBlock`]. A blocked
//! merge needs no resolver: the operator clears the cause (an operation in
//! progress, a checkout of the target branch, an overlapping change), and the
//! next tick's `merge_stage` lands it.

use crate::git::branch::resolve_target_branch;
use crate::models::stage::Stage;
use crate::orchestrator::core::{clear_status_line, Orchestrator};
use crate::orchestrator::signals::find_live_merge_session_for_stage;

use super::landing::Landing;

impl Orchestrator {
    /// Run `merge_stage` again for `stage`, which is `MergeBlocked` with a
    /// `merge_block`. A merge that lands follows the verified-merge path and
    /// its worktree is cleaned up, unless a resolver still runs in it: its
    /// exit cleans up then. A conflict moves the stage to `MergeConflict`; a
    /// block that is still there, or an error, leaves the stage for the next
    /// tick, and prints nothing.
    pub(super) fn retry_blocked_merge(&mut self, stage: &Stage) {
        let stage_id = stage.id.as_str();
        let target = resolve_target_branch(&self.config.base_branch, &self.config.repo_root);
        match self.land_stage_merge(stage_id, &target) {
            Landing::Merged => self.finish_retried_merge(stage_id, &target),
            Landing::Unproven => tracing::warn!(
                stage_id = %stage_id,
                "Retried merge has no ancestry proof; merged stays false"
            ),
            Landing::Failed(error) => tracing::warn!(
                stage_id = %stage_id,
                %error,
                "Retry of a blocked merge failed; retrying next tick"
            ),
            Landing::Held | Landing::Conflict(_) | Landing::Blocked(_) => {}
        }
    }

    fn finish_retried_merge(&mut self, stage_id: &str, target: &str) {
        clear_status_line();
        if self.resolver_may_be_live(stage_id) {
            eprintln!(
                "Stage '{stage_id}' merged after its block cleared; its worktree is removed \
                 once the merge resolver exits"
            );
            return;
        }
        self.cleanup_resolved_merge(stage_id, target);
    }

    /// Whether a merge resolver may still run in the stage worktree: one is
    /// tracked, a signal names a live one, or that could not be told.
    fn resolver_may_be_live(&self, stage_id: &str) -> bool {
        if self.active_sessions.contains_key(stage_id) {
            return true;
        }
        match find_live_merge_session_for_stage(stage_id, &self.config.work_dir) {
            Ok(live) => live.is_some(),
            Err(error) => {
                tracing::warn!(stage_id = %stage_id, %error, "Cannot tell whether a merge resolver runs");
                true
            }
        }
    }
}
