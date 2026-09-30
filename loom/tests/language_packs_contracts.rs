//! Contracts for stage language-packs: the TSX, JavaScript and wave-B/C grammar packs.
//!
//! Each test pins one rule of `doc/plans/briefs/source-graph-mechanism/design.md`
//! (sections 3.4, 4, 5.1 and 5.4).

use std::collections::BTreeSet;
use std::path::Path;

use loom::context::extract::{extract_file, registry, FileExtraction};
use loom::context::source_graph::{
    FileCoverage, NodeLanguage, SourceEdgeKind, SourceNode, SourceNodeKind,
};

const COMPONENT_SOURCE: &str =
    "import { Button } from \"./Button\";\nexport function App() {\n  return <Button label=\"x\" />;\n}\n";

fn extract(path: &str, source: &str) -> FileExtraction {
    extract_file(&registry(), Path::new(path), source.as_bytes())
}

/// Asserts the file went through a real grammar: full coverage, not the lexical fallback.
fn assert_full(path: &str, extraction: &FileExtraction) {
    assert_eq!(
        extraction.coverage,
        FileCoverage::Full,
        "{path} coverage; nodes: {:#?}",
        extraction.nodes
    );
}

/// The single node with `id`; panics on zero or many.
fn only_node<'a>(extraction: &'a FileExtraction, id: &str) -> &'a SourceNode {
    let nodes: Vec<_> = extraction.nodes.iter().filter(|n| n.id == id).collect();
    assert_eq!(
        nodes.len(),
        1,
        "expected one node {id}, got {:#?}",
        extraction.nodes
    );
    nodes[0]
}

#[test]
fn tsx_file_yields_symbol_nodes() {
    let extraction = extract("src/App.tsx", COMPONENT_SOURCE);

    assert_full("src/App.tsx", &extraction);
    let app = only_node(&extraction, "src/App.tsx#function:App");
    assert_eq!(app.kind, SourceNodeKind::Function, "node: {app:#?}");
    assert_eq!(app.scope, ["App"], "node: {app:#?}");
    assert_eq!(app.language, NodeLanguage::Tsx, "node: {app:#?}");
    assert_eq!(app.coverage, FileCoverage::Full, "node: {app:#?}");
}

/// Asserts a References edge from `<path>#function:App` to `Button`, sited on line 3.
fn assert_button_reference(path: &str) {
    let extraction = extract(path, COMPONENT_SOURCE);
    let from = format!("{path}#function:App");

    let edges: Vec<_> = extraction
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::References && e.from == from && e.symbol == "Button")
        .collect();
    assert_eq!(
        edges.len(),
        1,
        "expected one References edge {from} -> Button, got {:#?}",
        extraction.edges
    );
    let lines: Vec<usize> = edges[0].sites.iter().map(|s| s.line_start).collect();
    assert_eq!(lines, vec![3], "edge: {:#?}", edges[0]);
}

#[test]
fn jsx_element_becomes_reference_edge() {
    assert_button_reference("src/App.jsx");
    assert_button_reference("src/App.tsx");

    let jsx = extract("src/App.jsx", COMPONENT_SOURCE);
    let app = only_node(&jsx, "src/App.jsx#function:App");
    assert_eq!(app.language, NodeLanguage::JavaScript, "node: {app:#?}");
}

#[test]
fn commonjs_require_is_an_import_binding() {
    let extraction = extract(
        "src/main.js",
        "const util = require(\"./util\");\nfunction run() {\n  util.parse();\n}\n",
    );

    assert_full("src/main.js", &extraction);
    let import_edges: Vec<_> = extraction
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::Imports && e.symbol == "./util")
        .collect();
    assert_eq!(
        import_edges.len(),
        1,
        "expected one Imports edge for ./util, got {:#?}",
        extraction.edges
    );

    let bindings: Vec<_> = extraction
        .imports
        .iter()
        .filter(|b| b.path == "./util")
        .collect();
    assert_eq!(bindings.len(), 1, "imports: {:#?}", extraction.imports);
    let binding = bindings[0];
    assert_eq!(
        binding.alias.as_deref(),
        Some("util"),
        "binding: {binding:#?}"
    );
    assert!(!binding.glob, "binding: {binding:#?}");
    assert_eq!(binding.local_name(), Some("util"), "binding: {binding:#?}");
    assert_eq!(binding.site.line_start, 1, "binding: {binding:#?}");

    assert!(
        !extraction
            .edges
            .iter()
            .any(|e| e.kind == SourceEdgeKind::Calls && e.symbol == "require"),
        "require became a Calls edge: {:#?}",
        extraction.edges
    );
}

/// Function nodes of `extraction` whose scope ends with `tail`.
fn functions_ending_with<'a>(extraction: &'a FileExtraction, tail: &[&str]) -> Vec<&'a SourceNode> {
    extraction
        .nodes
        .iter()
        .filter(|n| {
            n.kind == SourceNodeKind::Function
                && n.scope.len() >= tail.len()
                && n.scope[n.scope.len() - tail.len()..]
                    .iter()
                    .zip(tail)
                    .all(|(segment, want)| segment == want)
        })
        .collect()
}

#[test]
fn wave_pack_dialects_extract_declarations() {
    let cases: [(&str, &str, NodeLanguage, &[&str]); 6] = [
        (
            "src/a/Widget.java",
            "package a;\npublic class Widget {\n  public void run() {}\n}\n",
            NodeLanguage::Java,
            &["Widget", "run"],
        ),
        (
            "src/Widget.cs",
            "namespace A {\n  public class Widget {\n    public void Run() {}\n  }\n}\n",
            NodeLanguage::CSharp,
            &["Widget", "Run"],
        ),
        (
            "lib/widget.rb",
            "class Widget\n  def run\n  end\nend\n",
            NodeLanguage::Ruby,
            &["Widget", "run"],
        ),
        (
            "src/Widget.php",
            "<?php\nclass Widget {\n  public function run() {}\n}\n",
            NodeLanguage::Php,
            &["Widget", "run"],
        ),
        (
            "src/run.c",
            "int run(void) { return 0; }\n",
            NodeLanguage::C,
            &["run"],
        ),
        (
            "src/widget.cpp",
            "class Widget {\n public:\n  void run() {}\n};\n",
            NodeLanguage::Cpp,
            &["Widget", "run"],
        ),
    ];

    for (path, source, language, tail) in cases {
        let extraction = extract(path, source);
        assert_full(path, &extraction);

        let nodes = functions_ending_with(&extraction, tail);
        assert_eq!(
            nodes.len(),
            1,
            "{path}: expected one Function ending with {tail:?}, got {:#?}",
            extraction.nodes
        );
        let node = nodes[0];
        assert_eq!(node.language, language, "{path}: node: {node:#?}");
        assert_eq!(node.coverage, FileCoverage::Full, "{path}: node: {node:#?}");
    }
}

#[test]
fn java_overloads_get_distinct_ids() {
    let path = "src/W.java";
    let extraction = extract(
        path,
        "class W {\n  void f(int a) {}\n  void f(String s) {}\n}\n",
    );

    assert_full(path, &extraction);
    let nodes = functions_ending_with(&extraction, &["W", "f"]);
    assert_eq!(nodes.len(), 2, "nodes: {:#?}", extraction.nodes);

    let ids: BTreeSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids.len(), 2, "overload ids collide: {ids:?}");

    let keys: BTreeSet<&str> = nodes.iter().map(|n| n.symbol_key.as_str()).collect();
    assert_eq!(
        keys.len(),
        1,
        "overloads do not share a symbol_key: {nodes:#?}"
    );
    let key = keys.into_iter().next().unwrap_or_default();
    assert!(
        !key.is_empty(),
        "overloads have an empty symbol_key: {nodes:#?}"
    );
    for node in &nodes {
        assert_ne!(node.id, key, "overload id is not disambiguated: {node:#?}");
        assert!(
            node.id.starts_with(key),
            "id {} does not extend its symbol_key {key}",
            node.id
        );
    }
}
