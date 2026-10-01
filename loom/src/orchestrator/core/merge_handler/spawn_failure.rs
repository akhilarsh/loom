//! Reporting a merge-resolver spawn failure: log it, and route the stage to
//! human review when the spawn was refused at the sandbox boundary rather
//! than failing for an ordinary operational reason (owner decision 4), or
//! when the failed spawn may have left a resolver running. Also the manual
//! merge steps the review reasons name, and the next step printed for a
//! stage whose auto-merge failed. Relocated out of `merge_handler.rs` to keep
//! it within its maintainability-ledger line budget.

use crate::git::branch::{branch_name_for_stage, resolve_target_branch};
use crate::models::failure::FailureType;
use crate::orchestrator::core::{clear_status_line, spawn_failure_type, Orchestrator};

use super::resolver_attempts::MAX_MERGE_RESOLVER_ATTEMPTS;
use super::resolver_stop::UnstoppedResolver;
use super::review_route::ReviewRoute;

impl Orchestrator {
    /// The manual way out of a merge the daemon escalated to `NeedsHumanReview`.
    ///
    /// `loom stage merge` and `loom stage retry` refuse `NeedsHumanReview`, so
    /// the steps name the path that works: `--force-complete` re-runs the merge.
    /// They carry no backticks, which status output mangles, and stay short
    /// enough for a typical stage id to fit the 200-char `MAX_INLINE_CHARS`
    /// line `loom status` shows the review reason in.
    pub(super) fn manual_merge_steps(&self, stage_id: &str) -> String {
        let target = resolve_target_branch(&self.config.base_branch, &self.config.repo_root);
        format!(
            "in .worktrees/{stage_id} merge {target}, resolve, commit, then run \
             loom stage human-review {stage_id} --force-complete"
        )
    }

    /// What a review reason adds when a merge resolver ran in the stage
    /// worktree: it may have left its merge in progress there.
    pub(super) fn in_progress_merge_note(&self, stage_id: &str) -> String {
        format!(
            "the worktree .worktrees/{stage_id} may hold an in-progress merge: run git merge \
             --abort there, or finish that merge by hand"
        )
    }

    /// Log a merge-resolver spawn failure. A sandbox refusal (owner decision
    /// 4) and a resolver that may still be running route the stage to review;
    /// any other failure is retried next tick. A possibly running resolver
    /// whose routing could not be saved is tracked instead, so the next pass
    /// waits for it rather than spawning a second one.
    pub(super) fn report_merge_spawn_failure(&mut self, stage_id: &str, e: anyhow::Error) {
        clear_status_line();
        eprintln!("Warning: Failed to spawn merge resolution session for '{stage_id}': {e}");
        let steps = self.manual_merge_steps(stage_id);
        if let Some(unstopped) = e.downcast_ref::<UnstoppedResolver>() {
            let session = unstopped.session.clone();
            let causes: Vec<String> = e.chain().skip(1).map(|cause| cause.to_string()).collect();
            let reason = format!(
                "{unstopped}; stop it by hand, then {steps}. Also, {}. Spawn error: {}",
                self.in_progress_merge_note(stage_id),
                causes.join(": ")
            );
            if self.route_merge_stage_to_review(stage_id, reason, None) == ReviewRoute::NotSaved {
                self.active_sessions.insert(stage_id.to_string(), session);
            }
            return;
        }
        if let Some(reason) = merge_spawn_block_reason(&e, &steps) {
            let failure_type = Some(FailureType::SandboxSetupFailure);
            self.route_merge_stage_to_review(stage_id, reason, failure_type);
        }
    }

    /// The next step `persist_merge_blocked` prints for a stage whose
    /// auto-merge failed. Outside manual mode the spawn loop retries a failed
    /// resolver spawn each tick, spawns at most `MAX_MERGE_RESOLVER_ATTEMPTS`
    /// resolvers, and routes the stage to human review when the gate holds its
    /// branch, the stage branch or the target branch is missing, or the budget
    /// is spent; a manual-mode run exits without retrying.
    pub(super) fn merge_blocked_hint(&self, stage_id: &str) -> String {
        let manual = format!("run from the stage worktree: loom stage merge {stage_id}");
        if self.config.manual_mode {
            return format!(
                "  Manual mode does not retry a failed merge-resolver spawn. If no resolver \
                 starts before this run exits,\n  {manual}"
            );
        }
        let max = MAX_MERGE_RESOLVER_ATTEMPTS;
        let branch = branch_name_for_stage(stage_id);
        let target = resolve_target_branch(&self.config.base_branch, &self.config.repo_root);
        format!(
            "  Loom spawns up to {max} merge resolvers for this stage, retrying a failed spawn \
             each tick.\n  It routes the stage to human review instead if branch {branch} \
             touches a control path ({}),\n  if branch {branch} or target branch {target} is \
             missing, or once all {max} resolvers have failed.\n  To merge by hand instead, \
             {manual}",
            Self::CONTROL_PATHS
        )
    }
}

/// `NeedsHumanReview` reason for a sandbox-refused merge-resolver spawn
/// (owner decision 4); `None` otherwise. The manual `steps` come before the
/// refusal text, which can run to several lines, so the 200-char line
/// `loom status` shows keeps them.
fn merge_spawn_block_reason(error: &anyhow::Error, steps: &str) -> Option<String> {
    if spawn_failure_type(error) != FailureType::SandboxSetupFailure {
        return None;
    }
    Some(format!(
        "sandbox preflight refused the merge resolver; {steps}. Refusal: {error:#}"
    ))
}

#[cfg(test)]
mod tests {
    use super::merge_spawn_block_reason;
    use crate::sandbox::preflight::SandboxPreflightRefusal;

    #[test]
    fn merge_spawn_preflight_refusal_blocks_and_only_it_does() {
        let refusal = SandboxPreflightRefusal::new(vec!["LOOM_BIN lies under /repo".to_string()]);
        let error = anyhow::Error::new(refusal);
        let reason = merge_spawn_block_reason(&error, "resolve by hand").unwrap();
        assert!(reason.contains("resolve by hand. Refusal: LOOM_BIN lies under /repo"));
        let other = anyhow::anyhow!("tmux could not create its socket directory");
        assert!(merge_spawn_block_reason(&other, "resolve by hand").is_none());
    }
}
