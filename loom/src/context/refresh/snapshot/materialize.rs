//! The resolved views a snapshot's readers ask for, built once per snapshot.

use anyhow::Result;

use super::{SnapshotAction, SnapshotOutcome};
use crate::context::graph_store::GraphStore;

/// Persist the resolved views readers of `outcome` will ask for, and keep the
/// one they ask for in `graph_store`, so a `loom map` process parses at most
/// one view file. A current view on disk is left unread. Advisory: a failure
/// is added to the reason, never turned into an unavailable snapshot.
pub(super) fn materialize_views(graph_store: &GraphStore, outcome: &mut SnapshotOutcome) {
    let servable = outcome.action != SnapshotAction::Unavailable || outcome.serving.is_some();
    if outcome.revision.is_empty() || !servable {
        return;
    }
    if let Err(error) = try_materialize(graph_store, outcome) {
        outcome.reason = format!(
            "{}; resolved view not materialized: {error:#}",
            outcome.reason
        );
    }
}

fn try_materialize(graph_store: &GraphStore, outcome: &SnapshotOutcome) -> Result<()> {
    let revision = outcome.revision.as_str();
    graph_store.materialize_view(revision, None)?;
    let Some((plan, stage)) = &outcome.overlay else {
        return Ok(());
    };
    if outcome.action != SnapshotAction::Reused {
        // The overlay view was resolved over the layers this snapshot rewrote.
        graph_store.discard_overlay_view(plan, stage);
    }
    graph_store.materialize_view(revision, Some((plan.as_str(), stage.as_str())))
}
