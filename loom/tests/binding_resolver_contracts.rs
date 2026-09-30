//! Contracts for stage binding-resolver: language-aware cross-file resolution.
//!
//! Each test pins one rule of section 7 of
//! `doc/plans/briefs/source-graph-mechanism/design.md`, resolving an in-memory
//! graph built by the real extractors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use loom::context::extract::{extract_file, registry};
use loom::context::graph_store::{FileEntry, ResolvedGraph};
use loom::context::resolve_graph;
use loom::context::source_graph::{EdgeProvenance, SourceEdge, SourceEdgeKind, UNRESOLVED_TARGET};

/// Extracts every `(path, source)` pair and resolves the resulting graph.
fn resolved(files: &[(&str, &str)]) -> ResolvedGraph {
    let extractors = registry();
    let mut entries = BTreeMap::new();
    for (path, source) in files {
        let bytes = source.as_bytes();
        let extraction = extract_file(&extractors, Path::new(path), bytes);
        entries.insert(
            (*path).to_string(),
            FileEntry::from_extraction(bytes, extraction),
        );
    }
    let mut graph = ResolvedGraph {
        base_revision: String::new(),
        overlaid: BTreeSet::new(),
        files: entries,
    };
    resolve_graph(&mut graph);
    graph
}

/// The single Calls edge leaving `from` for `symbol`; panics on zero or many.
fn only_call<'a>(graph: &'a ResolvedGraph, from: &str, symbol: &str) -> &'a SourceEdge {
    let edges: Vec<&SourceEdge> = graph
        .files
        .values()
        .flat_map(|entry| entry.edges.iter())
        .filter(|e| e.kind == SourceEdgeKind::Calls && e.from == from && e.symbol == symbol)
        .collect();
    assert_eq!(
        edges.len(),
        1,
        "expected exactly one Calls edge {from} -> {symbol}, got {edges:#?}"
    );
    edges[0]
}

fn assert_bound(edge: &SourceEdge, to: &str, provenance: EdgeProvenance) {
    assert_eq!(edge.to, to, "edge: {edge:#?}");
    assert_eq!(edge.provenance, provenance, "edge: {edge:#?}");
}

fn assert_unbound(edge: &SourceEdge, candidates: &[&str]) {
    assert_eq!(edge.provenance, EdgeProvenance::Syntax, "edge: {edge:#?}");
    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert_eq!(edge.candidates, candidates, "edge: {edge:#?}");
}

#[test]
fn import_alias_binds_the_aliased_definition() {
    let graph = resolved(&[
        ("src/util.ts", "export function parse() {}\n"),
        ("src/other.ts", "export function parse() {}\n"),
        (
            "src/main.ts",
            "import { parse as p } from \"./util\";\nexport function run() {\n  p();\n}\n",
        ),
    ]);

    let edge = only_call(&graph, "src/main.ts#function:run", "p");

    assert_bound(edge, "src/util.ts#function:parse", EdgeProvenance::Import);
}

#[test]
fn self_receiver_binds_across_files() {
    let graph = resolved(&[
        (
            "src/w.rs",
            "pub struct W;\nimpl W {\n    pub fn a(&self) {\n        self.b();\n    }\n}\n",
        ),
        ("src/w_ext.rs", "impl W {\n    pub fn b(&self) {}\n}\n"),
    ]);

    let edge = only_call(&graph, "src/w.rs#function:W::a", "b");

    assert_bound(edge, "src/w_ext.rs#function:W::b", EdgeProvenance::Receiver);
    assert_eq!(edge.receiver.as_deref(), Some("self"), "edge: {edge:#?}");
}

#[test]
fn dynamic_receiver_call_is_not_bound_by_name() {
    let graph = resolved(&[
        ("src/a.py", "class A:\n    def save(self):\n        pass\n"),
        ("src/b.py", "def run(obj):\n    obj.save()\n"),
    ]);

    let edge = only_call(&graph, "src/b.py#function:run", "save");

    assert_unbound(edge, &["src/a.py#function:A::save"]);
    assert_eq!(edge.receiver.as_deref(), Some("obj"), "edge: {edge:#?}");
}

#[test]
fn external_glob_import_refuses_unique_name() {
    let graph = resolved(&[
        (
            "src/a.rs",
            "use external_crate::*;\npub fn run() {\n    helper();\n}\n",
        ),
        ("src/b.rs", "pub fn helper() {}\n"),
    ]);

    let edge = only_call(&graph, "src/a.rs#function:run", "helper");

    assert_unbound(edge, &["src/b.rs#function:helper"]);
}

#[test]
fn names_never_bind_across_families() {
    let graph = resolved(&[
        ("src/p.py", "def run():\n    helper()\n"),
        ("src/g.go", "package g\nfunc helper() {}\n"),
    ]);

    let edge = only_call(&graph, "src/p.py#function:run", "helper");

    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}
