//! What the PHP extractor makes of declarations, `use` and `require`
//! statements, and calls, run over the fixtures in `tests/fixtures/source/php`.

use std::path::Path;

use crate::context::source_graph::{
    EdgeProvenance, FileCoverage, SourceEdge, SourceEdgeKind, SourceNodeKind, UNRESOLVED_TARGET,
};

use super::*;

const WIDGET: &str = include_str!("../../../../tests/fixtures/source/php/widget.php");
const NAMESPACED: &str = include_str!("../../../../tests/fixtures/source/php/namespaced.php");
const BRACED: &str = include_str!("../../../../tests/fixtures/source/php/braced.php");
const IMPORTS: &str = include_str!("../../../../tests/fixtures/source/php/imports.php");
const SYNTAX_ERROR: &str = include_str!("../../../../tests/fixtures/source/php/syntax_error.php");

fn extract(path: &str, source: &str) -> FileExtraction {
    PhpExtractor::new()
        .extract(Path::new(path), source.as_bytes())
        .unwrap()
}

fn sorted_ids(extraction: &FileExtraction) -> Vec<&str> {
    let mut ids: Vec<&str> = extraction.nodes.iter().map(|n| n.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

fn call<'a>(extraction: &'a FileExtraction, symbol: &str) -> &'a SourceEdge {
    let calls: Vec<_> = extraction
        .edges
        .iter()
        .filter(|edge| edge.kind == SourceEdgeKind::Calls && edge.symbol == symbol)
        .collect();
    assert_eq!(calls.len(), 1, "calls to {symbol}: {:#?}", extraction.edges);
    calls[0]
}

#[test]
fn extracts_declaration_ids_with_full_coverage() {
    let extraction = extract(
        "src/Widget.php",
        "<?php\nclass Widget {\n    public function run() {}\n}\n",
    );

    assert_eq!(
        sorted_ids(&extraction),
        vec![
            "src/Widget.php",
            "src/Widget.php#function:Widget::run",
            "src/Widget.php#type:Widget"
        ]
    );
    assert_eq!(extraction.coverage, FileCoverage::Full);
}

#[test]
fn a_trait_and_an_interface_are_declarations_and_a_bodiless_method_is_not() {
    let extraction = extract("src/w.php", WIDGET);

    assert_eq!(extraction.coverage, FileCoverage::Full);
    assert_eq!(
        sorted_ids(&extraction),
        vec![
            "src/w.php",
            "src/w.php#function:Loggable::format",
            "src/w.php#function:Loggable::log",
            "src/w.php#function:Widget::build",
            "src/w.php#function:Widget::make",
            "src/w.php#function:Widget::prepare",
            "src/w.php#function:Widget::run",
            "src/w.php#interface:Runnable",
            "src/w.php#type:Loggable",
            "src/w.php#type:Widget",
        ]
    );
}

#[test]
fn a_statement_namespace_is_one_dotted_segment() {
    let extraction = extract("src/w.php", NAMESPACED);

    let modules: Vec<_> = extraction
        .nodes
        .iter()
        .filter(|node| node.kind == SourceNodeKind::Module)
        .collect();
    assert_eq!(modules.len(), 1, "nodes: {:#?}", extraction.nodes);
    assert_eq!(modules[0].scope, ["App.Models"]);
    assert_eq!(modules[0].id, "src/w.php#module:App.Models");
}

#[test]
fn a_braced_namespace_scopes_its_classes() {
    let extraction = extract("src/w.php", BRACED);

    assert_eq!(
        sorted_ids(&extraction),
        vec![
            "src/w.php",
            "src/w.php#function:Outer.Inner::Widget::build",
            "src/w.php#function:Outer.Inner::Widget::run",
            "src/w.php#module:Outer.Inner",
            "src/w.php#type:Outer.Inner::Widget",
        ]
    );
    let edge = call(&extraction, "build");
    assert_eq!(edge.from, "src/w.php#function:Outer.Inner::Widget::run");
    assert_eq!(edge.to, "src/w.php#function:Outer.Inner::Widget::build");
    assert_eq!(edge.provenance, EdgeProvenance::Receiver);
    assert_eq!(edge.receiver.as_deref(), Some("self"));
}

#[test]
fn this_self_and_static_bind_to_members_of_the_enclosing_type() {
    let extraction = extract("src/w.php", WIDGET);
    let run = "src/w.php#function:Widget::run";

    for (symbol, receiver, target, line) in [
        ("prepare", "$this", "src/w.php#function:Widget::prepare", 23),
        ("build", "self", "src/w.php#function:Widget::build", 24),
        ("make", "static", "src/w.php#function:Widget::make", 25),
    ] {
        let edge = call(&extraction, symbol);
        assert_eq!(edge.from, run, "edge: {edge:#?}");
        assert_eq!(edge.to, target, "edge: {edge:#?}");
        assert_eq!(edge.provenance, EdgeProvenance::Receiver, "edge: {edge:#?}");
        assert_eq!(edge.receiver.as_deref(), Some(receiver), "edge: {edge:#?}");
        assert_eq!(edge.sites[0].line_start, line, "edge: {edge:#?}");
    }

    let format = call(&extraction, "format");
    assert_eq!(format.from, "src/w.php#function:Loggable::log");
    assert_eq!(format.to, "src/w.php#function:Loggable::format");
    assert_eq!(format.provenance, EdgeProvenance::Receiver);
}

#[test]
fn a_dynamic_receiver_and_a_scoped_class_call_stay_unresolved_with_their_receiver() {
    let extraction = extract("src/w.php", WIDGET);

    for (symbol, receiver) in [("go", "$target"), ("create", "Helper"), ("other", "H")] {
        let edge = call(&extraction, symbol);
        assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
        assert_eq!(edge.provenance, EdgeProvenance::Syntax, "edge: {edge:#?}");
        assert_eq!(edge.receiver.as_deref(), Some(receiver), "edge: {edge:#?}");
    }
    assert!(
        !extraction
            .edges
            .iter()
            .any(|edge| edge.kind == SourceEdgeKind::Calls && edge.symbol.contains("::")),
        "a scoped call made a qualified symbol: {:#?}",
        extraction.edges
    );
}

#[test]
fn an_imported_function_is_left_to_the_resolver_and_a_variable_call_is_not_captured() {
    let extraction = extract("src/w.php", WIDGET);

    let helper = call(&extraction, "helper_fn");
    assert_eq!(helper.to, UNRESOLVED_TARGET, "edge: {helper:#?}");
    assert_eq!(helper.receiver, None);
    assert!(
        !extraction
            .edges
            .iter()
            .any(|edge| edge.kind == SourceEdgeKind::Calls && edge.symbol.contains("name")),
        "edges: {:#?}",
        extraction.edges
    );
}

#[test]
fn use_clauses_bind_names_and_require_or_include_binds_a_glob() {
    let extraction = extract("src/w.php", IMPORTS);

    let bindings: Vec<_> = extraction
        .imports
        .iter()
        .map(|b| {
            (
                b.path.as_str(),
                b.alias.as_deref(),
                b.glob,
                b.local_name(),
                b.site.line_start,
            )
        })
        .collect();
    assert_eq!(
        bindings,
        vec![
            ("App\\Contracts\\Runnable", None, false, Some("Runnable"), 2),
            ("App\\Support\\Helper", Some("H"), false, Some("H"), 3),
            ("App\\Support\\helper_fn", None, false, Some("helper_fn"), 4),
            ("App\\Util\\Alpha", None, false, Some("Alpha"), 5),
            ("App\\Util\\Beta", Some("B"), false, Some("B"), 5),
            ("Plain", None, false, Some("Plain"), 6),
            ("vendor/autoload.php", None, true, None, 7),
            ("legacy.php", None, true, None, 8),
        ]
    );
    let edge_symbols: Vec<&str> = extraction
        .edges
        .iter()
        .filter(|edge| edge.kind == SourceEdgeKind::Imports)
        .map(|edge| edge.symbol.as_str())
        .collect();
    assert_eq!(edge_symbols.len(), 8, "edges: {:#?}", extraction.edges);
    assert!(edge_symbols.contains(&"App\\Util\\Beta"));
}

#[test]
fn a_syntax_error_yields_a_parse_error_and_no_symbols() {
    let extraction = extract("src/broken.php", SYNTAX_ERROR);

    assert!(
        matches!(extraction.coverage, FileCoverage::ParseError { .. }),
        "coverage: {:?}",
        extraction.coverage
    );
    assert_eq!(sorted_ids(&extraction), vec!["src/broken.php"]);
}
