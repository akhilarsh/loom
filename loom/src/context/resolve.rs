//! Cross-file symbol resolution and bounded reverse-impact traversal.
//!
//! Extraction is per file, so a call to a function defined elsewhere arrives
//! here as an [`EdgeProvenance::Syntax`] edge pointing at
//! [`UNRESOLVED_TARGET`](crate::context::source_graph::UNRESOLVED_TARGET): the
//! grammar that parsed the call site never saw the definition. This module is
//! the only place holding the whole graph at once, so it is the only place that
//! can turn some of those guesses into targets.
//!
//! **What it may claim.** An unresolved edge may be retargeted when the name it
//! recorded has exactly one definition in the entire graph, bound as
//! [`EdgeProvenance::UniqueName`] at its ceiling — above the extraction-time
//! ceiling, below certainty. A call written with a path — `crate::a::b()`,
//! `super::b()`, `Widget::new()` — is matched on that path, which carries more
//! evidence than the bare name at its end and binds as [`EdgeProvenance::Import`]:
//! the qualified spelling is tried against every node's scope, and failing that
//! the qualifier is matched onto files and the name is looked up only inside
//! them. A qualifier naming nothing here is a call into a dependency, so the
//! edge stays a gap — resolving it against a namesake elsewhere in the graph
//! would be a fabrication. Imports are matched against file paths by the rules
//! in `paths`, and both only ever resolve when exactly one candidate is left.
//!
//! **What it may never claim.**
//!
//! - *Never a complete call graph.* Resolution raises confidence where it can
//!   justify it and leaves the rest alone; [`ResolutionStats`] exists so a caller
//!   reports that residue instead of implying completeness.
//! - *Two candidates is ambiguity, not a coin flip.* Two definitions of one name
//!   leave the edge unresolved. Guessing would be indistinguishable from knowing,
//!   which is the failure this subsystem exists to avoid. The single exception is
//!   an `impl` block, which is indexed under its type's name without defining it
//!   (see `SymbolIndex::definitions`) — everything else contests.
//! - *An edge the extractor bound is never rewritten or downgraded.* A
//!   [`EdgeProvenance::Structural`] or [`EdgeProvenance::LocalName`] edge already
//!   has both endpoints from one file; name matching cannot improve on that.
//! - *Only a bound class the evidence supports.* Retargeting goes through
//!   `SourceEdge::bind`, which accepts only `Receiver`, `Import` and
//!   `UniqueName`.
//!
//! [`impact`](fn@impact) walks the result backwards and reports, for every node
//! reached, the confidence of the *weakest* edge on the path taken — so a chain
//! passing through one guess is never presented as stronger than that guess. It
//! lives in the private `impact` submodule and is re-exported here, so every
//! caller keeps one path: `crate::context::resolve::impact`.

use crate::context::graph_store::ResolvedGraph;
use crate::context::source_graph::{EdgeProvenance, SourceEdge, SourceEdgeKind};

mod impact;
mod neighbors;
mod paths;
mod symbols;

pub use impact::{impact, impact_with, ImpactHit, ImpactOptions, ImpactResult};
pub use neighbors::{direct_callees, direct_callers, Neighbor};
pub(crate) use symbols::node_names;
pub use symbols::SymbolIndex;

use paths::{import_candidates, PathIndex};

/// What resolution did, so a view can report it instead of implying completeness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolutionStats {
    /// Unresolved edges retargeted onto a unique definition.
    pub retargeted: usize,
    /// Edges left unresolved because two or more definitions matched.
    pub ambiguous: usize,
    /// Edges still pointing at `UNRESOLVED_TARGET` after resolution.
    pub unresolved: usize,
}

/// What resolution did to one edge.
enum Outcome {
    Retargeted,
    Ambiguous,
    Unresolved,
}

/// Point `edge` at `target` with `provenance`, through the one primitive allowed
/// to raise a syntax edge above the extraction-time ceiling.
///
/// That ceiling governs a guess made from a single file; this match was checked
/// against the whole graph, which is why the raise is justified. Going through
/// `SourceEdge::bind` rather than assigning the fields keeps the limit
/// structural: the confidence is the provenance's ceiling, and it refuses
/// outright on an edge that is not an unresolved `Syntax` edge, so resolution
/// can neither overwrite what the grammar proved nor relitigate its own earlier
/// decision.
///
/// Returns whether the edge was eligible; an ineligible edge is left untouched.
fn retarget(edge: &mut SourceEdge, target: &str, provenance: EdgeProvenance) -> bool {
    edge.bind(target, provenance)
}

/// What looking up one edge's written name found.
struct Found {
    /// Targets the edge could name.
    targets: Vec<String>,
    /// Whether the name was contested by two or more candidates *before* `impl`
    /// blocks were filtered out — so that a name genuinely fought over by
    /// several definitions still reports as ambiguous even when the filter
    /// leaves nothing behind.
    contested: bool,
    /// The evidence class a unique target binds with.
    provenance: EdgeProvenance,
}

/// What `edge`'s written name could refer to.
///
/// `from` is the path of the file the edge was extracted from, which is what a
/// relative path (`self::`, `super::`) is written against.
fn candidates_for(
    edge: &SourceEdge,
    from: &str,
    symbols: &SymbolIndex,
    paths: &PathIndex,
) -> Found {
    match edge.kind {
        SourceEdgeKind::Calls | SourceEdgeKind::References if !edge.symbol.is_empty() => {
            call_candidates(&edge.symbol, from, symbols, paths)
        }
        SourceEdgeKind::Imports if !edge.symbol.is_empty() => Found {
            targets: import_candidates(&edge.symbol, from, paths),
            contested: false,
            provenance: EdgeProvenance::Import,
        },
        // Containment, implementation and inheritance edges are emitted with both
        // endpoints inside one file. An unresolved one is a fact about the
        // extractor, not something a name match is entitled to repair.
        _ => Found {
            targets: Vec::new(),
            contested: false,
            provenance: EdgeProvenance::UniqueName,
        },
    }
}

/// Targets a call could name, strongest evidence first.
///
/// 1. The qualified spelling, longest first: `Widget::new` matches the `new`
///    inside `impl Widget` and no other, which is the whole reason the extractor
///    keeps the path.
/// 2. The qualifier as a module path, which scopes the search for the bare name
///    to the files it named: `crate::codex::run` is the `run` in `codex.rs`,
///    however many other `run`s the project has. A qualifier the graph cannot
///    place — `String::from` names a type this project never defined — leaves
///    the edge unresolved, because every `from` in the graph is then a namesake
///    rather than a candidate.
///
/// `Self::helper()` reaches this function as the bare symbol `helper`: the Rust
/// extractor captures `Self` as the call's receiver and, where the enclosing
/// type defines the member, binds the edge at extraction with Receiver
/// provenance. The edge carries no `Self::` qualifier for this function to
/// resolve.
fn call_candidates(symbol: &str, from: &str, symbols: &SymbolIndex, paths: &PathIndex) -> Found {
    let segments: Vec<&str> = symbol.split("::").collect();
    let Some((name, qualifier)) = segments.split_last().filter(|(_, rest)| !rest.is_empty()) else {
        return by_name(symbols, symbol, EdgeProvenance::UniqueName);
    };

    for start in 0..qualifier.len() {
        let spelling = segments[start..].join("::");
        if !symbols.lookup(&spelling).is_empty() {
            return by_name(symbols, &spelling, EdgeProvenance::Import);
        }
    }

    // The files the qualifier names decide. A name none of them defines is
    // somewhere the path did not point — an external crate, or an item
    // re-exported from further down — so the definitions elsewhere in the graph
    // are namesakes rather than rival candidates, and the edge stays a gap.
    let module = import_candidates(&qualifier.join("::"), from, paths);
    let inside = symbols.definitions_in(name, &module);
    Found {
        contested: inside.len() >= 2,
        targets: inside,
        provenance: EdgeProvenance::Import,
    }
}

/// Definitions of one written name, with its contest status.
fn by_name(symbols: &SymbolIndex, name: &str, provenance: EdgeProvenance) -> Found {
    Found {
        targets: symbols.definitions(name),
        contested: symbols.contested(name),
        provenance,
    }
}

/// Resolve one unresolved syntax edge against the whole-graph indexes.
fn resolve_edge(
    edge: &mut SourceEdge,
    from: &str,
    symbols: &SymbolIndex,
    paths: &PathIndex,
) -> Outcome {
    let found = candidates_for(edge, from, symbols, paths);
    match found.targets.as_slice() {
        // `retarget` refusing means the edge was not the unresolved syntax
        // edge this pass is meant to act on. Report what actually happened
        // rather than claiming a retarget that did not occur.
        [only] if *only != edge.from => {
            if retarget(edge, only, found.provenance) {
                Outcome::Retargeted
            } else {
                Outcome::Unresolved
            }
        }
        // A lone candidate that is the edge's own origin resolves nothing, and
        // neither does an empty set — but either can still be a contested name.
        [_] | [] if !found.contested => Outcome::Unresolved,
        _ => Outcome::Ambiguous,
    }
}

/// Rewrite the unresolved edges of `graph` in place where the evidence justifies it.
pub fn resolve_graph(graph: &mut ResolvedGraph) -> ResolutionStats {
    let symbols = SymbolIndex::build(graph);
    let paths = PathIndex::build(graph);
    let mut stats = ResolutionStats::default();

    for (path, entry) in graph.files.iter_mut() {
        for edge in entry.edges.iter_mut() {
            // Every class but `Syntax` already has both endpoints, from
            // something stronger than a name match. Never rewritten, and never
            // counted: they are not part of the residue.
            if edge.provenance != EdgeProvenance::Syntax || !edge.is_unresolved() {
                continue;
            }
            match resolve_edge(edge, path, &symbols, &paths) {
                Outcome::Retargeted => stats.retargeted += 1,
                Outcome::Ambiguous => {
                    stats.ambiguous += 1;
                    stats.unresolved += 1;
                }
                Outcome::Unresolved => stats.unresolved += 1,
            }
        }
    }

    stats
}

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "resolve/tests_resolve.rs"]
mod tests;

#[cfg(test)]
#[path = "resolve/tests_qualified.rs"]
mod tests_qualified;
