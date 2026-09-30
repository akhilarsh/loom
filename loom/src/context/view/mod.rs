//! The resolved view: a source graph after cross-file resolution, with the
//! identity of what produced it and the lookup keys every resolved edge
//! consulted.
//!
//! [`build_cold`] resolves a graph of extraction-time edges from scratch.
//! [`relink`] reaches the same result from a previous view by re-resolving only
//! the edges a changed, added or removed file can affect; its output is equal
//! to the cold build's byte for byte in [`canonical_bytes`]. The view holds no
//! name or adjacency index: queries scan the graph in one linear pass.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::context::graph_store::ResolvedGraph;
use crate::context::resolve::ResolutionStats;
use crate::context::store::canonical_json;

mod build;
mod deps;
mod identity;
mod incremental;
mod store;

pub use build::build_cold;
pub use deps::DependencyIndex;
pub use identity::{ViewIdentity, RESOLVER_VERSION};
pub use incremental::relink;

/// A resolved graph together with what produced it.
///
/// Every map is a `BTreeMap` and every list is sorted or in extraction order,
/// so two equal views serialize identically. Equality ignores
/// [`Self::origin`], as [`canonical_bytes`] does.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedView {
    pub identity: ViewIdentity,
    /// The graph with every resolvable edge resolved.
    pub graph: ResolvedGraph,
    /// [`resolution_stats`](crate::context::resolve::resolution_stats) of `graph`.
    pub stats: ResolutionStats,
    /// Lookup key to the edges whose resolution consulted it.
    pub deps: DependencyIndex,
    /// How this process came by the view. Never serialized, so it never
    /// reaches [`canonical_bytes`].
    #[serde(skip)]
    pub origin: ViewOrigin,
}

impl PartialEq for ResolvedView {
    /// A view read back from disk equals the one resolved in process.
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.graph == other.graph
            && self.stats == other.stats
            && self.deps == other.deps
    }
}

/// Whether a view was read from disk or resolved in this process; `loom map`
/// reports it as `"view"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewOrigin {
    /// Parsed from a persisted view file. The default, because deserializing
    /// is the only way a view is made without resolving.
    #[default]
    Materialized,
    /// Resolved by [`build_cold`] or [`relink`].
    Built,
}

impl ViewOrigin {
    /// `"materialized"` or `"built"`, as `loom map` prints it.
    pub fn as_str(self) -> &'static str {
        match self {
            ViewOrigin::Materialized => "materialized",
            ViewOrigin::Built => "built",
        }
    }
}

/// The canonical JSON of the whole view, [`ResolvedView::origin`] excluded.
pub fn canonical_bytes(view: &ResolvedView) -> Result<Vec<u8>> {
    canonical_json(view).map(String::into_bytes)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_equivalence;

#[cfg(test)]
mod tests_store;

#[cfg(test)]
mod tests_store_cache;

#[cfg(test)]
mod tests_transitions;
