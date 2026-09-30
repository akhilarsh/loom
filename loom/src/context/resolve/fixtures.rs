//! Hand-built graph fixtures shared by the resolution and traversal tests.
//!
//! Every graph in these tests is written out by hand: the point of the tests is
//! the honesty contract, so what evidence exists must be visible in the fixture
//! rather than produced by a real extractor run.

use std::path::{Path, PathBuf};

use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::resolve::ResolutionStats;
use crate::context::source_graph::{
    node_id, syntax_confidence, EdgeProvenance, FileCoverage, ImportBinding, NodeLanguage,
    SourceEdge, SourceEdgeKind, SourceNode, SourceNodeKind, Span,
};

pub(crate) fn file_node(path: &str) -> SourceNode {
    SourceNode {
        id: path.to_string(),
        kind: SourceNodeKind::File,
        path: PathBuf::from(path),
        scope: Vec::new(),
        span: Span::default(),
        signature: String::new(),
        body_hash: "sha256:node".to_string(),
        language: NodeLanguage::Rust,
        parser_version: "test".to_string(),
        coverage: FileCoverage::Full,
        symbol_key: String::new(),
    }
}

/// Canonical id of a one-segment symbol, matching what the extractors emit.
pub(crate) fn scoped_id(path: &str, kind: SourceNodeKind, name: &str) -> String {
    node_id(Path::new(path), kind, &[name.to_string()])
}

pub(crate) fn func_id(path: &str, name: &str) -> String {
    scoped_id(path, SourceNodeKind::Function, name)
}

pub(crate) fn scoped_node(path: &str, kind: SourceNodeKind, name: &str) -> SourceNode {
    nested_node(path, kind, &[name])
}

/// A node whose scope has more than one segment — a function inside an `impl`
/// or a `mod` — which is what a qualified call has to match against.
pub(crate) fn nested_node(path: &str, kind: SourceNodeKind, scope: &[&str]) -> SourceNode {
    let scope: Vec<String> = scope.iter().map(|segment| segment.to_string()).collect();
    SourceNode {
        id: node_id(Path::new(path), kind, &scope),
        kind,
        signature: format!("{kind} {}", scope.join("::")),
        scope,
        ..file_node(path)
    }
}

/// A file entry holding a file node plus the symbol nodes given.
fn entry_of(path: &str, symbols: Vec<SourceNode>, edges: Vec<SourceEdge>) -> FileEntry {
    let mut nodes = vec![file_node(path)];
    nodes.extend(symbols);
    FileEntry {
        content_hash: "sha256:file".to_string(),
        nodes,
        edges,
        coverage: FileCoverage::Full,
        imports: Vec::new(),
    }
}

/// A file entry whose symbol nodes are given as `(kind, name)` pairs — for the
/// cases where the kind is the point, such as a type beside its `impl` blocks.
pub(crate) fn mixed_file(
    path: &str,
    symbols: &[(SourceNodeKind, &str)],
    edges: Vec<SourceEdge>,
) -> FileEntry {
    let nodes = symbols
        .iter()
        .map(|(kind, name)| scoped_node(path, *kind, name))
        .collect();
    entry_of(path, nodes, edges)
}

/// A file entry holding one function per nested scope, for the cases where the
/// scope is the point: `["Widget", "new"]` is `new` inside `impl Widget`.
pub(crate) fn nested_file(path: &str, scopes: &[&[&str]], edges: Vec<SourceEdge>) -> FileEntry {
    let nodes = scopes
        .iter()
        .map(|scope| nested_node(path, SourceNodeKind::Function, scope))
        .collect();
    entry_of(path, nodes, edges)
}

/// A file entry holding a file node plus one function node per name.
pub(crate) fn source_file(path: &str, symbols: &[&str], edges: Vec<SourceEdge>) -> FileEntry {
    let nodes = symbols
        .iter()
        .map(|name| scoped_node(path, SourceNodeKind::Function, name))
        .collect();
    entry_of(path, nodes, edges)
}

pub(crate) fn graph_of(files: Vec<(&str, FileEntry)>) -> ResolvedGraph {
    ResolvedGraph {
        files: files
            .into_iter()
            .map(|(path, entry)| (path.to_string(), entry))
            .collect(),
        ..ResolvedGraph::default()
    }
}

/// A graph of function-bearing files: `(path, function names, edges)`.
pub(crate) fn graph_from(files: Vec<(&str, &[&str], Vec<SourceEdge>)>) -> ResolvedGraph {
    graph_of(
        files
            .into_iter()
            .map(|(path, symbols, edges)| (path, source_file(path, symbols, edges)))
            .collect(),
    )
}

/// An edge with fields set exactly as written — the only way to build one the
/// extractor constructors would refuse (a `Structural` edge left unresolved, or
/// a syntax edge above the extraction ceiling).
pub(crate) fn edge_at(
    from: &str,
    to: &str,
    kind: SourceEdgeKind,
    provenance: EdgeProvenance,
    confidence: f32,
) -> SourceEdge {
    SourceEdge {
        from: from.to_string(),
        to: to.to_string(),
        kind,
        provenance,
        confidence,
        symbol: String::new(),
        sites: Vec::new(),
        candidates: Vec::new(),
        receiver: None,
    }
}

/// A `Syntax` edge of `kind` leaving `from` and naming `symbol`, with no target
/// yet: what an extractor emits for a name it could not bind in its own file.
pub(crate) fn unresolved_edge(
    from: impl Into<String>,
    kind: SourceEdgeKind,
    symbol: impl Into<String>,
) -> SourceEdge {
    SourceEdge::syntax(from, kind, symbol, Span::default(), syntax_confidence(kind))
}

/// A resolved edge as an extractor emits it for a same-file hit: containment is
/// `Structural`, every other kind `LocalName`.
pub(crate) fn local_edge(
    from: impl Into<String>,
    to: impl Into<String>,
    kind: SourceEdgeKind,
    symbol: impl Into<String>,
) -> SourceEdge {
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

/// An unresolved edge of `kind` leaving `from`, naming `symbol`.
pub(crate) fn seeking(from: &str, kind: SourceEdgeKind, symbol: &str) -> Vec<SourceEdge> {
    vec![unresolved_edge(from, kind, symbol)]
}

/// An import binding of `name` (`None`: the whole module) from `path`, bound
/// under `alias` when given.
pub(crate) fn binding(path: &str, name: Option<&str>, alias: Option<&str>) -> ImportBinding {
    ImportBinding {
        path: path.to_string(),
        name: name.map(str::to_string),
        alias: alias.map(str::to_string),
        glob: false,
        site: Span::default(),
    }
}

/// A glob import of `path`: `use x::*`, `using A.B;`, `#include "x.h"`.
pub(crate) fn glob_binding(path: &str) -> ImportBinding {
    ImportBinding {
        glob: true,
        ..binding(path, None, None)
    }
}

/// Canonical id of a symbol with the given scope.
pub(crate) fn nested_id(path: &str, kind: SourceNodeKind, scope: &[&str]) -> String {
    nested_node(path, kind, scope).id
}

/// Symbol nodes of a hand-built file, as `(kind, scope)` pairs.
pub(crate) type Symbols<'a> = &'a [(SourceNodeKind, &'a [&'a str])];

/// A file entry for a hand-built dialect fixture: a file node plus one node per
/// `(kind, scope)`, the edges extracted from it, and its import bindings.
pub(crate) fn dialect_file(
    path: &str,
    symbols: Symbols,
    edges: Vec<SourceEdge>,
    imports: Vec<ImportBinding>,
) -> FileEntry {
    let nodes = symbols
        .iter()
        .map(|(kind, scope)| nested_node(path, *kind, scope))
        .collect();
    FileEntry {
        imports,
        ..entry_of(path, nodes, edges)
    }
}

/// A graph of entries built by this module, each keyed by its file node's path.
pub(crate) fn graph_of_files(entries: Vec<FileEntry>) -> ResolvedGraph {
    ResolvedGraph {
        files: entries
            .into_iter()
            .map(|entry| (entry.nodes[0].id.clone(), entry))
            .collect(),
        ..ResolvedGraph::default()
    }
}

/// The three residue counts a resolution pass reports, in the order they read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Residue {
    pub(crate) retargeted: usize,
    pub(crate) ambiguous: usize,
    pub(crate) unresolved: usize,
}

/// Stats equal a residue when the three counts match, whatever their
/// `by_provenance`: a test about the per-provenance counts asserts them itself.
impl PartialEq<Residue> for ResolutionStats {
    fn eq(&self, residue: &Residue) -> bool {
        (self.retargeted, self.ambiguous, self.unresolved)
            == (residue.retargeted, residue.ambiguous, residue.unresolved)
    }
}

pub(crate) fn expected_stats(retargeted: usize, ambiguous: usize, unresolved: usize) -> Residue {
    Residue {
        retargeted,
        ambiguous,
        unresolved,
    }
}
