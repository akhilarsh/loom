//! Cross-file symbol resolution and bounded reverse-impact traversal.
//!
//! Extraction is per file, so a call to a function defined elsewhere arrives
//! here as an [`EdgeProvenance::Syntax`] edge pointing at
//! [`UNRESOLVED_TARGET`](crate::context::source_graph::UNRESOLVED_TARGET): the
//! grammar that parsed the call site never saw the definition. This module is
//! the only place holding the whole graph at once, so it is the only place that
//! can turn some of those gaps into targets.
//!
//! **What it acts on.** Only an unresolved `Syntax` call, reference or import
//! with no candidates. An edge the extractor bound is never rewritten, and a
//! candidate set found at extraction is same-file ambiguity, which is final.
//! Binding goes through `SourceEdge::bind`, which accepts only `Receiver`,
//! `Import` and `UniqueName`, at the confidence ceiling of that class.
//!
//! **Families.** Every lookup is keyed by the resolution family of the edge's
//! file (`ecmascript` joins TypeScript, TSX and JavaScript; `c` joins C and
//! C++; every other dialect is its own), so nothing ever binds across
//! families: a Python call never lands on a Go definition.
//!
//! **Rules**, in order, for a call or reference (the first that decides wins):
//!
//! 1. *Qualified spelling* (`a::b::n`, `A.B.n`): matched against node scopes,
//!    longest first; failing that, the qualifier is mapped onto module files by
//!    the dialect's path conventions and `n` is looked up inside them. A
//!    qualifier naming nothing here is a call into a dependency and stays a gap
//!    unless an import binds its first segment (rule 4).
//! 2. *Other receiver* (`obj.m()`, `ns.m()`): when the receiver is an import's
//!    local name, `m` is looked up in the files the import names. Any other
//!    receiver is a value of unknown type and is never bound.
//! 3. *Self receiver* (`self.m()`, `this.m()`): the members named `m` of the
//!    enclosing type, wherever its parts are declared.
//! 4. *Named or aliased import*: the imported name in the files the import
//!    names.
//! 5. *Package scope*: the other files of the edge's Go or Java package
//!    directory, or of its C# or PHP namespace.
//! 6. *Glob imports*: the files of every glob import that resolves.
//! 7. *Unique name*: the only definition of the name in the family.
//!
//! Rules 1 to 6 bind with [`EdgeProvenance::Import`] (rule 3 with
//! [`EdgeProvenance::Receiver`]) when exactly one definition is left, and rule 7
//! with [`EdgeProvenance::UniqueName`]. Imports edges bind to the one file their
//! module spec names.
//!
//! **Refusals.** A named import whose module matches no file, or a glob import
//! that does not resolve, is a name bound outside this graph: rule 7 may no
//! longer bind it, because the definition that happens to be unique here is a
//! namesake. The refused edge still records its same-family namesakes as
//! candidates, so impact analysis keeps its recall.
//!
//! **Candidate sets.** Two or more definitions are never a coin flip: the edge
//! stays unresolved and lists them as `candidates`, sorted, at most
//! [`MAX_CANDIDATES`](crate::context::source_graph::MAX_CANDIDATES); beyond that
//! the list stays empty. A dynamic receiver lists even a single namesake as a
//! candidate and binds nothing. A lone candidate that is the edge's own origin
//! resolves nothing. An `impl` block is indexed under its type's name without
//! defining it, so it never contests the type.
//!
//! **Never a complete call graph.** Resolution raises confidence where the
//! evidence justifies it and leaves the rest alone; [`ResolutionStats`] exists
//! so a caller reports that residue instead of implying completeness.
//!
//! **Recording.** [`resolve_graph_recording`] and [`resolve_edges`] return the
//! lookup keys each edge consulted, and [`touched_keys`] the keys a file's nodes
//! answer to, so an incremental relink re-resolves exactly the edges a changed
//! file can affect.
//!
//! [`impact`](fn@impact) walks the result backwards and reports, for every node
//! reached, the confidence of the *weakest* edge on the path taken — so a chain
//! passing through one guess is never presented as stronger than that guess.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::context::graph_store::ResolvedGraph;
use crate::context::source_graph::{EdgeProvenance, SourceEdge, SourceEdgeKind};

mod bindings;
mod impact;
mod neighbors;
mod paths;
mod receivers;
mod record;
mod rules;
mod symbols;

pub use impact::{impact, impact_with, ImpactHit, ImpactOptions, ImpactResult};
pub use neighbors::{direct_callees, direct_callers, direct_references, Neighbor};
pub(crate) use record::node_names;
pub use record::{touched_keys, EdgeKeys, EdgeRef};
pub use symbols::SymbolIndex;

use rules::{Indexes, Outcome, Resolution};

/// What the edges of a graph say after resolution, so a view can report the
/// residue instead of implying completeness. A pure count over the edges: the
/// same graph always gives the same stats.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionStats {
    /// Edges bound with `Import`, `Receiver` or `UniqueName` provenance: every
    /// cross-file bind, plus the `Receiver` binds extraction made itself.
    pub retargeted: usize,
    /// Edges left unresolved with a candidate set.
    pub ambiguous: usize,
    /// Edges pointing at `UNRESOLVED_TARGET`, candidates or not.
    pub unresolved: usize,
    /// Every edge but `Contains`, counted by its provenance's `as_str()`.
    pub by_provenance: BTreeMap<String, usize>,
}

/// Resolve every eligible edge of `graph` in place; returns
/// [`resolution_stats`] of the result.
pub fn resolve_graph(graph: &mut ResolvedGraph) -> ResolutionStats {
    resolve_graph_recording(graph).0
}

/// [`resolve_graph`], also returning the lookup keys each resolved edge
/// consulted.
pub fn resolve_graph_recording(graph: &mut ResolvedGraph) -> (ResolutionStats, EdgeKeys) {
    let edges: BTreeSet<EdgeRef> = graph
        .files
        .iter()
        .flat_map(|(path, entry)| {
            entry
                .edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| rules::eligible(edge))
                .map(move |(index, _)| EdgeRef {
                    path: path.clone(),
                    index,
                })
        })
        .collect();
    let keys = resolve_edges(graph, &edges);
    (resolution_stats(graph), keys)
}

/// Resolve only `edges`, against indexes over the whole graph, and return the
/// keys each consulted. A reference to no edge, or to one resolution does not
/// act on, is skipped. Each edge's outcome depends only on the graph's nodes
/// and import bindings, never on another edge, so resolving a subset gives
/// those edges exactly what a full resolution would.
pub fn resolve_edges(graph: &mut ResolvedGraph, edges: &BTreeSet<EdgeRef>) -> EdgeKeys {
    let resolutions = resolve_selected(graph, edges);
    let mut consulted = EdgeKeys::new();
    for (edge_ref, Resolution { outcome, keys }) in resolutions {
        let entry = graph.files.get_mut(&edge_ref.path);
        if let Some(edge) = entry.and_then(|entry| entry.edges.get_mut(edge_ref.index)) {
            apply(edge, outcome);
        }
        consulted.insert(edge_ref, keys);
    }
    consulted
}

/// Count the edges of `graph` as they stand.
pub fn resolution_stats(graph: &ResolvedGraph) -> ResolutionStats {
    let mut stats = ResolutionStats::default();
    for edge in graph.edges() {
        stats.unresolved += usize::from(edge.is_unresolved());
        stats.ambiguous += usize::from(!edge.candidates.is_empty());
        stats.retargeted += usize::from(matches!(
            edge.provenance,
            EdgeProvenance::Import | EdgeProvenance::Receiver | EdgeProvenance::UniqueName
        ));
        if edge.kind != SourceEdgeKind::Contains {
            let provenance = edge.provenance.as_str().to_string();
            *stats.by_provenance.entry(provenance).or_default() += 1;
        }
    }
    stats
}

/// Decide every selected edge against one read-only view of the graph, before
/// any of them is rewritten.
fn resolve_selected(
    graph: &ResolvedGraph,
    edges: &BTreeSet<EdgeRef>,
) -> Vec<(EdgeRef, Resolution)> {
    let indexes = Indexes::build(graph);
    edges
        .iter()
        .filter_map(|edge_ref| {
            let entry = graph.files.get(&edge_ref.path)?;
            let edge = entry.edges.get(edge_ref.index)?;
            rules::eligible(edge).then(|| {
                let resolution = rules::resolve(edge, &edge_ref.path, entry, &indexes);
                (edge_ref.clone(), resolution)
            })
        })
        .collect()
}

/// Write one outcome onto its edge. Candidates keep the extraction-time
/// provenance and confidence; only a bind raises them.
fn apply(edge: &mut SourceEdge, outcome: Outcome) {
    match outcome {
        Outcome::Bound(target, provenance) => {
            let bound = edge.bind(target, provenance);
            debug_assert!(bound, "only eligible edges are resolved");
        }
        Outcome::Candidates(ids) => edge.candidates = ids,
        Outcome::Unresolved => {}
    }
}

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "resolve/tests_resolve.rs"]
mod tests;

#[cfg(test)]
#[path = "resolve/tests_import_edges.rs"]
mod tests_import_edges;

#[cfg(test)]
#[path = "resolve/tests_qualified.rs"]
mod tests_qualified;

#[cfg(test)]
#[path = "resolve/tests_paths.rs"]
mod tests_paths;

#[cfg(test)]
#[path = "resolve/tests_paths_packages.rs"]
mod tests_paths_packages;

#[cfg(test)]
#[path = "resolve/tests_rules.rs"]
mod tests_rules;

#[cfg(test)]
#[path = "resolve/tests_recording.rs"]
mod tests_recording;

#[cfg(test)]
#[path = "resolve/tests_candidates.rs"]
mod tests_candidates;
