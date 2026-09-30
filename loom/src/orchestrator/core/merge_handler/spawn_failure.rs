//! Reporting a merge-resolver spawn failure: log it, and route the stage to
//! human review when the spawn was refused at the sandbox boundary rather
//! than failing for an ordinary operational reason (owner decision 4). Also
//! the manual merge steps this and the exhausted-resolver escalation both
//! name. Relocated out of `merge_handler.rs` to keep it within its
//! maintainability-ledger line budget — unrelated to the merge gate itself.

use crate::models::failure::FailureType;
use crate::orchestrator::core::{clear_status_line, spawn_failure_type, Orchestrator};

impl Orchestrator {
    /// The manual way out of a merge the daemon escalated to `NeedsHumanReview`.
    ///
    /// `loom stage merge` and `loom stage retry` refuse `NeedsHumanReview`, so
    /// the steps name the path that works: `--force-complete` re-runs the merge.
    /// They carry no backticks, which status output mangles, and stay short
    /// enough for a typical stage id to fit the 200-char `MAX_INLINE_CHARS`
    /// line `loom status` shows the review reason in.
    pub(super) fn manual_merge_steps(&self, stage_id: &str) -> String {
        let target = crate::git::branch::resolve_target_branch(
            &self.config.base_branch,
            &self.config.repo_root,
        );
        format!(
            "in .worktrees/{stage_id} merge {target}, resolve, commit, then run \
             loom stage human-review {stage_id} --force-complete"
        )
    }

    /// Log a merge-resolver spawn failure; block only on a sandbox refusal (owner decision 4).
    pub(super) fn report_merge_spawn_failure(&mut self, stage_id: &str, e: anyhow::Error) {
        clear_status_line();
        eprintln!("Warning: Failed to spawn merge resolution session for '{stage_id}': {e}");
        let steps = self.manual_merge_steps(stage_id);
        if let Some(reason) = merge_spawn_block_reason(&e, &steps) {
            self.route_to_human_review(stage_id, reason, Some(FailureType::SandboxSetupFailure));
        }
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
