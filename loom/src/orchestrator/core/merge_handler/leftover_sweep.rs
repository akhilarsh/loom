//! The daemon's sweep over merged stages that still hold a worktree or a
//! branch. Cleanup after a merge is deferred while a session still runs for
//! the stage, and a stage merged by the CLI from inside its own worktree
//! leaves the worktree behind. Nothing else revisits either, so each tick
//! retries them here, through the same `MergeLifecycle::cleanup` door that
//! refuses unless the stage's work is contained in the target branch.

use crate::models::stage::{StageStatus, StageType};
use crate::orchestrator::core::persistence::Persistence;
use crate::orchestrator::core::{clear_status_line, Orchestrator};
use crate::orchestrator::merge_lifecycle::CleanupOutcome;
use crate::plan::StageNode;

impl Orchestrator {
    /// Clean up the worktree and branch of every `Completed` and `merged`
    /// non-knowledge stage not yet settled this daemon session.
    ///
    /// Candidates come from the in-memory graph; a stage file is read only for
    /// a candidate. A deferred cleanup is tried again next tick. Any other
    /// outcome settles the stage: it was cleaned, had nothing to clean, or was
    /// refused or failed and reported, which a retry would repeat unchanged.
    pub(in crate::orchestrator::core) fn sweep_merged_leftovers(&mut self) {
        let graph = &self.graph;
        // A stage that left `Completed` and `merged` (re-queued, say) is swept
        // afresh when it merges again.
        self.settled_leftovers
            .retain(|id| is_merged(graph.get_node(id)));
        let candidates: Vec<String> = graph
            .all_nodes()
            .into_iter()
            .filter(|node| is_merged(Some(node)) && !self.settled_leftovers.contains(&node.id))
            .map(|node| node.id.clone())
            .collect();
        for stage_id in candidates {
            if self.sweep_one_leftover(&stage_id) {
                self.settled_leftovers.insert(stage_id);
            }
        }
    }

    /// Sweep `stage_id`; returns whether it is settled. A stage file that is
    /// unreadable or no longer `Completed` and `merged` stays unsettled.
    fn sweep_one_leftover(&self, stage_id: &str) -> bool {
        let stage = match self.load_stage(stage_id) {
            Ok(stage) => stage,
            Err(error) => {
                tracing::debug!(stage_id, %error, "Leftover sweep could not read the stage");
                return false;
            }
        };
        if stage.status != StageStatus::Completed || !stage.merged {
            return false;
        }
        // Knowledge stages run in the main checkout and own no worktree.
        if stage.stage_type == StageType::Knowledge {
            return true;
        }
        !matches!(
            self.cleanup_already_merged(stage_id),
            CleanupOutcome::Deferred { .. }
        )
    }
}

fn is_merged(node: Option<&StageNode>) -> bool {
    node.is_some_and(|node| node.status == StageStatus::Completed && node.merged)
}

/// Make a failed or refused deferred cleanup visible on the daemon's console.
///
/// `MergeLifecycle::cleanup` already logs at `warn`, but the daemon's tracing
/// goes to stderr nobody watches; the stage is Completed and merged either
/// way, so the only thing lost by silence is the worktree the user later
/// finds still on disk. A deferral is silent: a later attempt finishes it.
pub(super) fn report_deferred_cleanup(stage_id: &str, outcome: &CleanupOutcome) {
    let (verb, detail) = match outcome {
        CleanupOutcome::Failed(error) => ("failed", error),
        CleanupOutcome::Refused { reason } => ("refused", reason),
        CleanupOutcome::Done(_) | CleanupOutcome::NothingToDo | CleanupOutcome::Deferred { .. } => {
            return
        }
    };
    clear_status_line();
    eprintln!("Warning: deferred cleanup for stage '{stage_id}' {verb}: {detail}");
    eprintln!("  Clean up manually with: loom worktree remove {stage_id}");
}

#[cfg(test)]
#[path = "leftover_sweep_tests.rs"]
mod tests;
