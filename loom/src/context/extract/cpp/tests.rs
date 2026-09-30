//! What the C++ extractor emits: qualified and nested definitions, namespaces,
//! overloads, includes and calls, and what it leaves as a gap.

use std::path::Path;

use super::*;
use crate::context::source_graph::{EdgeProvenance, FileCoverage, SourceEdgeKind, SourceNode};

const C_HEADER_H: &str = include_str!("../../../../tests/fixtures/source/cpp/c_header.h");
const EXTERN_C_GUARD_H: &str =
    include_str!("../../../../tests/fixtures/source/cpp/extern_c_guard.h");
const OVERLOADS: &str = include_str!("../../../../tests/fixtures/source/cpp/overloads.cpp");
const THIS_CALL: &str = include_str!("../../../../tests/fixtures/source/cpp/this_call.cpp");
const NAMESPACES: &str = include_str!("../../../../tests/fixtures/source/cpp/namespaces.cpp");
const SYNTAX_ERROR: &str = include_str!("../../../../tests/fixtures/source/cpp/syntax_error.cpp");

fn extraction(path: &str, source: &str) -> FileExtraction {
    CppExtractor::new()
        .extract(Path::new(path), source.as_bytes())
        .unwrap()
}

fn node_ids(extraction: &FileExtraction) -> Vec<&str> {
    let mut ids: Vec<&str> = extraction.nodes.iter().map(|n| n.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

fn node<'a>(extraction: &'a FileExtraction, id: &str) -> &'a SourceNode {
    extraction
        .nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap_or_else(|| panic!("no node {id}: {:#?}", extraction.nodes))
}

/// `(from, symbol, provenance, to)` of every `Calls` edge, sorted.
fn calls(extraction: &FileExtraction) -> Vec<(&str, &str, EdgeProvenance, &str)> {
    let mut calls: Vec<_> = extraction
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::Calls)
        .map(|e| {
            (
                e.from.as_str(),
                e.symbol.as_str(),
                e.provenance,
                e.to.as_str(),
            )
        })
        .collect();
    calls.sort_unstable();
    calls
}

#[test]
fn a_c_header_parses_as_cpp_with_its_declarations() {
    let header = extraction("include/c_header.h", C_HEADER_H);

    assert_eq!(header.coverage, FileCoverage::Full);
    assert_eq!(
        node_ids(&header),
        vec![
            "include/c_header.h",
            "include/c_header.h#function:is_open",
            "include/c_header.h#type:Handle",
        ]
    );
}

/// A guard whose `{` and `}` sit in different `#ifdef` blocks is not parseable:
/// the file is a reported parse error, never a half tree.
#[test]
fn an_extern_c_guard_split_across_ifdefs_is_a_parse_error() {
    let guarded = extraction("include/guard.h", EXTERN_C_GUARD_H);

    assert!(
        matches!(guarded.coverage, FileCoverage::ParseError { .. }),
        "coverage: {:?}",
        guarded.coverage
    );
    assert_eq!(node_ids(&guarded), vec!["include/guard.h"]);
}

#[test]
fn overloads_are_distinct_nodes_sharing_a_symbol_key() {
    let overloads = extraction("src/overloads.cpp", OVERLOADS);

    for (scope, key) in [
        (
            "Printer::print",
            "src/overloads.cpp#function:Printer::print",
        ),
        ("emit", "src/overloads.cpp#function:emit"),
    ] {
        let twins: Vec<_> = overloads
            .nodes
            .iter()
            .filter(|n| n.kind == SourceNodeKind::Function && n.scope.join("::") == scope)
            .collect();
        assert_eq!(twins.len(), 2, "{scope}: {:#?}", overloads.nodes);
        assert_ne!(twins[0].id, twins[1].id, "{scope} ids collide");
        assert!(twins
            .iter()
            .all(|n| n.symbol_key == key && n.id.starts_with(key)));
    }
}

#[test]
fn an_out_of_line_definition_scopes_under_its_qualifier() {
    let widget = extraction("src/this_call.cpp", THIS_CALL);

    assert_eq!(
        node_ids(&widget),
        vec![
            "src/this_call.cpp",
            "src/this_call.cpp#function:Widget::run",
            "src/this_call.cpp#function:Widget::skip",
            "src/this_call.cpp#function:Widget::step",
            "src/this_call.cpp#type:Widget",
        ]
    );
    let skip = node(&widget, "src/this_call.cpp#function:Widget::skip");
    assert_eq!(skip.scope, ["Widget", "skip"]);
    assert_eq!(skip.span.line_start, 13);
}

#[test]
fn this_and_bare_calls_bind_to_members_in_and_out_of_the_class() {
    let widget = extraction("src/this_call.cpp", THIS_CALL);
    let step = "src/this_call.cpp#function:Widget::step";

    let run = "src/this_call.cpp#function:Widget::run";
    let skip = "src/this_call.cpp#function:Widget::skip";
    let mut expected = vec![
        (run, "step", EdgeProvenance::LocalName, step),
        (run, "step", EdgeProvenance::Receiver, step),
        (skip, "step", EdgeProvenance::Receiver, step),
    ];
    expected.sort_unstable();
    assert_eq!(calls(&widget), expected);
    let receivers: Vec<Option<&str>> = widget
        .edges
        .iter()
        .filter(|e| e.provenance == EdgeProvenance::Receiver)
        .map(|e| e.receiver.as_deref())
        .collect();
    assert_eq!(receivers, vec![Some("this"), Some("this")]);
}

#[test]
fn a_free_prototype_is_a_reference_and_a_member_declaration_is_not() {
    let source = "namespace s {\nint total(int v);\nint &pick(int v);\n}\n\
                  class W {\n    void run();\n};\n";
    let header = extraction("include/s.h", source);

    let references: Vec<(&str, &str)> = header
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::References)
        .map(|e| (e.from.as_str(), e.symbol.as_str()))
        .collect();
    assert_eq!(
        references,
        vec![
            ("include/s.h#module:s", "pick"),
            ("include/s.h#module:s", "total"),
        ]
    );
}

/// In a function body `Foo w(x);` is a variable initialised from `x`, which
/// the grammar also reads as a function declarator: it is no prototype. A
/// template or friend prototype still is.
#[test]
fn a_local_direct_initialisation_is_not_a_prototype() {
    let source = "int total(int v);\ntemplate <typename T> T twice(T v);\n\
                  class W {\n    friend int peek(W &w);\n};\n\
                  void run(int x) {\n    Foo w(x);\n}\n";
    let unit = extraction("src/run.cpp", source);

    let mut references: Vec<&str> = unit
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::References)
        .map(|e| e.symbol.as_str())
        .collect();
    references.sort_unstable();
    assert_eq!(references, vec!["peek", "total", "twice"]);
}

#[test]
fn includes_are_glob_bindings() {
    let widget = extraction("src/this_call.cpp", THIS_CALL);

    assert_eq!(widget.imports.len(), 1);
    assert_eq!(widget.imports[0].path, "widget.h");
    assert!(widget.imports[0].glob);
}

#[test]
fn a_nested_namespace_is_one_module_with_one_scope_segment() {
    let ns = extraction("src/namespaces.cpp", NAMESPACES);

    let outer = node(&ns, "src/namespaces.cpp#module:outer::inner");
    assert_eq!(outer.kind, SourceNodeKind::Module);
    assert_eq!(outer.scope, ["outer::inner"]);
    let helper = node(&ns, "src/namespaces.cpp#function:outer::inner::helper");
    assert_eq!(helper.scope, ["outer::inner", "helper"]);
}

#[test]
fn a_definition_qualified_by_several_scopes_gets_every_segment() {
    let ns = extraction("src/namespaces.cpp", NAMESPACES);

    let start = node(&ns, "src/namespaces.cpp#function:lib::Engine::start");
    assert_eq!(start.scope, ["lib", "Engine", "start"]);
    assert_eq!(start.span.line_start, 12);
    // The class and its prototype: a prototype adds no node.
    assert_eq!(
        ns.nodes
            .iter()
            .filter(|n| n.scope.last().is_some_and(|s| s == "start"))
            .count(),
        1
    );
}

#[test]
fn a_namespace_qualified_call_keeps_its_spelling_and_a_template_call_drops_its_arguments() {
    let ns = extraction("src/namespaces.cpp", NAMESPACES);
    let calls = calls(&ns);

    assert!(
        calls.contains(&(
            "src/namespaces.cpp#function:lib::Engine::start",
            "outer::inner::helper",
            EdgeProvenance::LocalName,
            "src/namespaces.cpp#function:outer::inner::helper",
        )),
        "calls: {calls:#?}"
    );
    assert!(
        calls.contains(&(
            "src/namespaces.cpp#function:use",
            "identity",
            EdgeProvenance::LocalName,
            "src/namespaces.cpp#function:identity",
        )),
        "calls: {calls:#?}"
    );
}

#[test]
fn a_call_on_another_receiver_is_left_unbound() {
    let source = "struct A { void m() {} void n(A &other) { other.m(); } };\n";
    let a = extraction("src/a.cpp", source);
    let call = a
        .edges
        .iter()
        .find(|e| e.kind == SourceEdgeKind::Calls)
        .unwrap();

    assert_eq!(call.receiver.as_deref(), Some("other"));
    assert_eq!(call.provenance, EdgeProvenance::Syntax);
}

#[test]
fn a_template_definition_is_captured_and_a_defaulted_member_is_not() {
    let source = "template <typename T>\nclass Box {\npublic:\n  Box() = default;\n  T get() { return value; }\n  T value;\n};\n";
    let boxed = extraction("src/box.hpp", source);

    assert_eq!(
        node_ids(&boxed),
        vec![
            "src/box.hpp",
            "src/box.hpp#function:Box::get",
            "src/box.hpp#type:Box"
        ]
    );
}

#[test]
fn a_syntax_error_yields_a_parse_error_and_no_symbols() {
    let broken = extraction("src/syntax_error.cpp", SYNTAX_ERROR);

    assert!(
        matches!(broken.coverage, FileCoverage::ParseError { .. }),
        "coverage: {:?}",
        broken.coverage
    );
    assert_eq!(node_ids(&broken), vec!["src/syntax_error.cpp"]);
}

/// A prototype is a `References` edge, and the capability says so.
#[test]
fn the_references_capability_matches_the_edges_emitted() {
    let unit = extraction("src/a.cpp", "int area(int side);\n");

    assert!(unit
        .edges
        .iter()
        .any(|edge| edge.kind == SourceEdgeKind::References));
    assert!(CppExtractor::new().capabilities().references);
}
