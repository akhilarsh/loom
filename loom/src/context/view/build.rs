//! The cold build: resolve every edge of a graph from scratch.

use crate::context::graph_store::ResolvedGraph;
use crate::context::resolve::resolve_graph_recording;

use super::{DependencyIndex, ResolvedView, ViewIdentity, ViewOrigin};

/// Resolve `graph`, which holds extraction-time edges, into a view stamped
/// with `identity`.
pub fn build_cold(mut graph: ResolvedGraph, identity: ViewIdentity) -> ResolvedView {
    count_resolution_run();
    let (stats, keys) = resolve_graph_recording(&mut graph);
    ResolvedView {
        identity,
        graph,
        stats,
        deps: DependencyIndex::from_edge_keys(keys),
        origin: ViewOrigin::Built,
    }
}

/// Note one resolution run, so tests can tell a served view from a rebuilt
/// one. Compiles to nothing outside tests.
pub(super) fn count_resolution_run() {
    #[cfg(test)]
    RESOLUTION_RUNS.with(|runs| runs.set(runs.get() + 1));
}

#[cfg(test)]
thread_local! {
    /// Per thread, because lib tests run on parallel threads.
    static RESOLUTION_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Resolution runs `build_cold` and `relink` made on this thread.
#[cfg(test)]
pub(crate) fn resolution_runs() -> usize {
    RESOLUTION_RUNS.with(std::cell::Cell::get)
}
