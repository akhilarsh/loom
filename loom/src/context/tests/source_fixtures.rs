//! Shared hand-built fixtures for source-graph tests: `SourceNode`s, a
//! `ResolvedGraph` built without touching the filesystem, and a
//! `Channel::Source` candidate — mirroring `coverage.rs`'s fixture style.
//!
//! `pub(super)` makes every item here visible to any sibling module under
//! `context::tests` via `use super::source_fixtures::...`.

use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::rank::RankedCandidate;
use crate::context::schema::{
    Channel, ChunkId, FileCoverage, NodeLanguage, SelectionReason, SourceNode, SourceNodeKind, Span,
};
use crate::context::source_graph::{syntax_confidence, EdgeProvenance, SourceEdge, SourceEdgeKind};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Build a `SourceNode` with every field explicit.
pub(super) fn node(
    id: &str,
    path: &str,
    scope: &[&str],
    signature: &str,
    kind: SourceNodeKind,
    coverage: FileCoverage,
) -> SourceNode {
    SourceNode {
        id: id.to_string(),
        kind,
        path: PathBuf::from(path),
        scope: scope.iter().map(|segment| segment.to_string()).collect(),
        span: Span::default(),
        signature: signature.to_string(),
        body_hash: format!("sha256:{id}"),
        language: NodeLanguage::Rust,
        parser_version: "test+v1".to_string(),
        coverage,
        symbol_key: String::new(),
    }
}

/// A `SourceNodeKind::Function` node with `FileCoverage::Full` — the common
/// case most tests need, so they don't repeat both arguments. Callers that
/// need real line numbers mutate `.span.line_start`/`.span.line_end` after
/// building, since `Span`'s fields are public.
pub(super) fn full_node(id: &str, path: &str, scope: &[&str], signature: &str) -> SourceNode {
    node(
        id,
        path,
        scope,
        signature,
        SourceNodeKind::Function,
        FileCoverage::Full,
    )
}

/// Build a `ResolvedGraph` from `(path, nodes)` pairs, one `FileEntry` per
/// path. `FileEntry::coverage` is unused by `rank_source` (only each node's
/// own `coverage` is), so it is always `Full` here.
pub(super) fn graph(files: Vec<(&str, Vec<SourceNode>)>) -> ResolvedGraph {
    let mut map = BTreeMap::new();
    for (path, nodes) in files {
        map.insert(
            path.to_string(),
            FileEntry {
                content_hash: "sha256:abc".to_string(),
                nodes,
                edges: Vec::new(),
                coverage: FileCoverage::Full,
                imports: Vec::new(),
            },
        );
    }
    ResolvedGraph {
        base_revision: "rev1".to_string(),
        overlaid: Default::default(),
        files: map,
    }
}

/// A `ResolvedGraph` holding exactly one node, keyed by its own path.
pub(super) fn graph_with_node(source_node: SourceNode) -> ResolvedGraph {
    let path = source_node.path.to_string_lossy().into_owned();
    graph(vec![(path.as_str(), vec![source_node])])
}

/// A `Channel::Source` candidate, mirroring `pack.rs`'s `candidate()` helper.
pub(super) fn source_candidate(id: &str, score: f32, token_count: usize) -> RankedCandidate {
    RankedCandidate {
        id: ChunkId::from(id),
        channel: Channel::Source,
        score,
        reasons: vec![SelectionReason::ExactPath],
        token_count,
        matched_term_count: 0,
        confidence_ceiling: None,
        via: None,
    }
}

/// A resolved edge as an extractor emits it for a same-file hit: containment is
/// `Structural`, every other kind `LocalName`.
pub(super) fn local_edge(from: &str, to: &str, kind: SourceEdgeKind, symbol: &str) -> SourceEdge {
    match kind {
        SourceEdgeKind::Contains => SourceEdge::structural(from, to, symbol),
        _ => SourceEdge::bound(
            from,
            to,
            kind,
            symbol,
            Span::default(),
            EdgeProvenance::LocalName,
        ),
    }
}

/// An edge of `kind` that names `symbol` but no target.
pub(super) fn unresolved_edge(from: &str, kind: SourceEdgeKind, symbol: &str) -> SourceEdge {
    SourceEdge::syntax(from, kind, symbol, Span::default(), syntax_confidence(kind))
}
