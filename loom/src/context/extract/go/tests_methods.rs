//! A Go method is scoped under its receiver type and, since Go cannot call a
//! method by its bare name, a bare call never binds to it.

use std::path::Path;

use super::*;
use crate::context::source_graph::{
    EdgeProvenance, SourceEdge, SourceEdgeKind, SourceNode, SourceNodeKind,
};

fn extraction(source: &str) -> FileExtraction {
    GoExtractor::new()
        .extract(Path::new("p/w.go"), source.as_bytes())
        .unwrap()
}

fn node<'a>(extraction: &'a FileExtraction, id: &str) -> &'a SourceNode {
    extraction
        .nodes
        .iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("no node {id}: {:#?}", extraction.nodes))
}

/// The single `Calls` edge for `symbol`.
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
fn every_receiver_shape_scopes_the_method_under_its_type() {
    for receiver in [
        "w Widget",
        "w *Widget",
        "Widget",
        "*Widget",
        "w *Widget[T]",
        "w Widget[T]",
    ] {
        let source = format!("package p\n\nfunc ({receiver}) run() {{}}\n");
        let extraction = extraction(&source);

        let method = node(&extraction, "p/w.go#function:Widget::run");
        assert_eq!(method.scope, ["Widget", "run"], "{receiver}");
        let parents: Vec<&str> = extraction
            .edges
            .iter()
            .filter(|e| e.kind == SourceEdgeKind::Contains && e.to == method.id)
            .map(|e| e.from.as_str())
            .collect();
        assert_eq!(parents, ["p/w.go"], "{receiver}");
    }
}

#[test]
fn a_bare_call_never_binds_to_a_method() {
    let extraction = extraction(
        "package p\n\ntype Widget struct{}\n\nfunc (w *Widget) run() {}\n\nfunc Start() {\n\trun()\n}\n",
    );

    let edge = call(&extraction, "run");
    assert_eq!(edge.from, "p/w.go#function:Start");
    assert_eq!(edge.provenance, EdgeProvenance::Syntax, "edge: {edge:#?}");
    assert!(edge.is_unresolved(), "edge: {edge:#?}");
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}

#[test]
fn a_method_and_a_function_of_one_name_get_distinct_ids_and_a_bare_call_binds_the_function() {
    let extraction = extraction(
        "package p\n\nfunc run() {}\n\nfunc (w Widget) run() {}\n\nfunc (w Widget) start() {\n\trun()\n}\n",
    );

    node(&extraction, "p/w.go#function:run");
    node(&extraction, "p/w.go#function:Widget::run");
    let edge = call(&extraction, "run");
    assert_eq!(edge.from, "p/w.go#function:Widget::start");
    assert_eq!(edge.to, "p/w.go#function:run", "edge: {edge:#?}");
    assert_eq!(
        edge.provenance,
        EdgeProvenance::LocalName,
        "edge: {edge:#?}"
    );
}

#[test]
fn every_named_type_declares_a_type_and_only_an_interface_literal_an_interface() {
    let extraction = extraction(
        "package p\n\ntype stack []int\ntype ids map[string]int\ntype handler func()\n\
         type celsius float64\ntype grid [4]int\ntype feed chan int\ntype ref *stack\n\
         type set[T comparable] map[T]struct{}\ntype local = stack\ntype remote = io.Reader\n\
         type Widget struct{}\ntype Runner interface{ Run() }\ntype Doer = interface{ Do() }\n",
    );

    let mut declared: Vec<String> = extraction
        .nodes
        .iter()
        .filter(|node| !matches!(node.kind, SourceNodeKind::File | SourceNodeKind::Module))
        .map(|node| format!("{}:{}", node.kind, node.scope.join("::")))
        .collect();
    declared.sort_unstable();

    let types = [
        "Widget", "celsius", "feed", "grid", "handler", "ids", "local", "ref", "remote", "set",
        "stack",
    ];
    let mut expected: Vec<String> = types.iter().map(|name| format!("type:{name}")).collect();
    expected.extend(["interface:Doer".to_string(), "interface:Runner".to_string()]);
    expected.sort_unstable();
    assert_eq!(declared, expected, "{:#?}", extraction.nodes);
}

#[test]
fn a_method_on_a_named_slice_type_is_owned_by_a_type_node() {
    let extraction = extraction(
        "package p\n\ntype stack []int\n\nfunc (s *stack) len() int {\n\treturn len(*s)\n}\n",
    );

    node(&extraction, "p/w.go#type:stack");
    node(&extraction, "p/w.go#function:stack::len");
    let edge = call(&extraction, "len");
    assert_eq!(edge.from, "p/w.go#function:stack::len");
    assert!(
        edge.is_unresolved(),
        "the builtin, never the method: {edge:#?}"
    );
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}

#[test]
fn go_reports_receiver_capture() {
    assert!(GoExtractor::new().capabilities().receivers);
}
