//! Which of the four graph states a `SnapshotOutcome` reports, and the stale
//! base a failed build serves.

use super::{SnapshotAction, SnapshotOutcome};
use crate::context::freshness::GraphState;
use crate::context::graph_store::GraphStore;
use crate::context::refresh::clean_generation;

impl SnapshotOutcome {
    /// `Current` for a reused, updated or rebuilt layer; `Stale` when a failed
    /// build serves an older base; `NeverBuilt` when it failed over a known
    /// HEAD with no base to serve; `Unavailable` when the working tree could
    /// not be inspected (no HEAD, so `revision` is empty).
    pub fn state(&self) -> GraphState {
        match self.action {
            SnapshotAction::Reused | SnapshotAction::Updated | SnapshotAction::Rebuilt => {
                GraphState::Current
            }
            SnapshotAction::Unavailable if self.serving.is_some() => GraphState::Stale,
            SnapshotAction::Unavailable if self.revision.is_empty() => GraphState::Unavailable,
            SnapshotAction::Unavailable => GraphState::NeverBuilt,
        }
    }
}

/// When `outcome` is a failed build over a known HEAD, point it at the newest
/// usable base so readers still get an answer, labelled stale. The action stays
/// `Unavailable`: callers that require a fresh graph still refuse it. A base
/// that cannot be listed or read counts as no base.
pub(super) fn serve_stale_base(
    graph_store: &GraphStore,
    mut outcome: SnapshotOutcome,
) -> SnapshotOutcome {
    if outcome.action != SnapshotAction::Unavailable || outcome.revision.is_empty() {
        return outcome;
    }
    if let Ok(Some(base)) = graph_store.load_newest_base() {
        outcome.generation = clean_generation(&base.revision);
        outcome.serving = Some(base.revision.clone());
        outcome.revision = base.revision;
    }
    outcome
}
