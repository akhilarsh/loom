//! The per-tick retry of a merge held by a typed [`MergeBlock`]. A blocked
//! merge needs no resolver: the operator clears the cause (an operation in
//! progress, a checkout of the target branch, an overlapping change), and the
//! next tick's `merge_stage` lands it.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::git::branch::resolve_target_branch;
use crate::git::{blocked_merge_inputs, MergeBlock};
use crate::models::stage::Stage;
use crate::orchestrator::core::{clear_status_line, Orchestrator};
use crate::orchestrator::signals::find_live_merge_session_for_stage;

use super::landing::Landing;

/// The paths of the stage's current `UncommittedOverlap` block, which the
/// retry fingerprint watches: an ignored file among them is not in status.
fn overlap_paths(stage: &Stage) -> Vec<String> {
    match &stage.merge.block {
        Some(MergeBlock::UncommittedOverlap { paths }) => paths.clone(),
        _ => Vec::new(),
    }
}

/// A memo entry older than this is ignored: a safety net for inputs the
/// fingerprint cannot see (git config, attributes, permissions).
const MEMO_MAX_AGE: Duration = Duration::from_secs(10 * 60);

/// The least time between two retries of a merge that ended
/// `FastForwardRefused`; each attempt writes a merge commit object.
const REFUSED_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Whether any `watched` path is a directory. A directory's modification
/// time does not move when a file deeper inside is removed, so the
/// fingerprint cannot watch it.
fn watches_directory(repo_root: &Path, watched: &[String]) -> bool {
    watched
        .iter()
        .any(|path| std::fs::symlink_metadata(repo_root.join(path)).is_ok_and(|meta| meta.is_dir()))
}

impl Orchestrator {
    /// Run `merge_stage` again for `stage`, which is `MergeBlocked` with a
    /// `merge_block`; see [`Self::retry_blocked_merge_at`].
    pub(super) fn retry_blocked_merge(&mut self, stage: &Stage) {
        self.retry_blocked_merge_at(stage, Instant::now());
    }

    /// Run `merge_stage` again for `stage` at time `now`. A merge that lands
    /// follows the verified-merge path and its worktree is cleaned up, unless
    /// a resolver still runs in it: its exit cleans up then. A conflict moves
    /// the stage to `MergeConflict`; a block that is still there, or an
    /// error, leaves the stage for the next tick, and prints nothing.
    ///
    /// A retry is skipped while the inputs the block depends on are the ones
    /// the last blocked attempt saw, for at most ten minutes: an attempt
    /// writes git objects even when the target cannot advance. An attempt
    /// that watches a directory is never skipped, and a `FastForwardRefused`
    /// one is retried at most once a minute.
    pub(super) fn retry_blocked_merge_at(&mut self, stage: &Stage, now: Instant) {
        let stage_id = stage.id.as_str();
        if self.refused_retry_pending(stage_id, now) {
            tracing::debug!(stage_id = %stage_id, "Fast-forward refusal retried too recently; skipped");
            return;
        }
        let target = resolve_target_branch(&self.config.base_branch, &self.config.repo_root);
        let watched = overlap_paths(stage);
        let inputs = if watches_directory(&self.config.repo_root, &watched) {
            None
        } else {
            blocked_merge_inputs(&self.config.repo_root, stage_id, &target, &watched).ok()
        };
        if inputs.is_some_and(|inputs| self.memo_holds(stage_id, inputs, now)) {
            tracing::debug!(stage_id = %stage_id, "Merge block inputs unchanged; retry skipped");
            return;
        }
        let landing = self.land_stage_merge(stage_id, &target);
        self.remember_blocked_inputs(stage_id, inputs, &landing, now);
        match landing {
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

    /// Whether the memo holds `inputs` for the stage, recorded under
    /// [`MEMO_MAX_AGE`] before `now`.
    fn memo_holds(&self, stage_id: &str, inputs: u64, now: Instant) -> bool {
        self.blocked_merge_inputs
            .get(stage_id)
            .is_some_and(|(seen, at)| {
                *seen == inputs && now.saturating_duration_since(*at) < MEMO_MAX_AGE
            })
    }

    /// Whether the stage's last attempt ended `FastForwardRefused` less than
    /// [`REFUSED_RETRY_INTERVAL`] before `now`.
    fn refused_retry_pending(&self, stage_id: &str, now: Instant) -> bool {
        self.refused_merge_attempts
            .get(stage_id)
            .is_some_and(|at| now.saturating_duration_since(*at) < REFUSED_RETRY_INTERVAL)
    }

    /// Keep `inputs`, computed before the attempt, when it ended in a block
    /// that only a change of those inputs can clear (a blocked attempt
    /// changes nothing in the main checkout); `FastForwardRefused` is timed
    /// instead, since a transient lock leaves no trace in the inputs. Every
    /// other outcome drops both entries.
    fn remember_blocked_inputs(
        &mut self,
        stage_id: &str,
        inputs: Option<u64>,
        landing: &Landing,
        now: Instant,
    ) {
        self.blocked_merge_inputs.remove(stage_id);
        self.refused_merge_attempts.remove(stage_id);
        match (inputs, landing) {
            (
                Some(inputs),
                Landing::Blocked(
                    MergeBlock::UncommittedOverlap { .. }
                    | MergeBlock::TargetCheckedOutElsewhere { .. }
                    | MergeBlock::OperatorOperation { .. },
                ),
            ) => {
                self.blocked_merge_inputs
                    .insert(stage_id.to_string(), (inputs, now));
            }
            (_, Landing::Blocked(MergeBlock::FastForwardRefused { .. })) => {
                self.refused_merge_attempts
                    .insert(stage_id.to_string(), now);
            }
            _ => {}
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

#[cfg(test)]
#[path = "blocked_retry_tests.rs"]
mod tests;
