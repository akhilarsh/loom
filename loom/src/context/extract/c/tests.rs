//! What the C extractor emits: declarations, includes and calls, and what it
//! leaves as a gap.

use std::path::Path;

use super::*;
use crate::context::source_graph::{EdgeProvenance, FileCoverage, SourceEdgeKind};

const API_C: &str = include_str!("../../../../tests/fixtures/source/c/api.c");
const API_H: &str = include_str!("../../../../tests/fixtures/source/c/api.h");
const MACRO_C: &str = include_str!("../../../../tests/fixtures/source/c/macro_generated.c");
const IFDEF_C: &str = include_str!("../../../../tests/fixtures/source/c/ifdef_branches.c");
const DISPATCH_C: &str = include_str!("../../../../tests/fixtures/source/c/dispatch.c");
const SYNTAX_ERROR_C: &str = include_str!("../../../../tests/fixtures/source/c/syntax_error.c");

fn extraction(path: &str, source: &str) -> FileExtraction {
    CExtractor::new()
        .extract(Path::new(path), source.as_bytes())
        .unwrap()
}

fn node_ids(extraction: &FileExtraction) -> Vec<&str> {
    let mut ids: Vec<&str> = extraction.nodes.iter().map(|n| n.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

#[test]
fn a_prototype_in_a_header_is_not_a_definition() {
    let header = extraction("src/api.h", API_H);
    assert_eq!(header.coverage, FileCoverage::Full);
    assert_eq!(node_ids(&header), vec!["src/api.h"]);
}

#[test]
fn a_prototype_is_a_reference_to_the_function_it_declares() {
    let source = "#include <stdio.h>\nint area(int side);\nchar *name(void);\n\
                  int main(void)\n{\n    return area(2);\n}\n";
    let main = extraction("src/main.c", source);

    assert_eq!(
        node_ids(&main),
        vec!["src/main.c", "src/main.c#function:main"]
    );
    let references: Vec<(&str, &str, EdgeProvenance, usize)> = main
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::References)
        .map(|e| {
            let line = e.sites[0].line_start;
            (e.from.as_str(), e.symbol.as_str(), e.provenance, line)
        })
        .collect();
    assert_eq!(
        references,
        vec![
            ("src/main.c", "area", EdgeProvenance::Syntax, 2),
            ("src/main.c", "name", EdgeProvenance::Syntax, 3),
        ]
    );
}

#[test]
fn definitions_and_typedefs_are_one_node_each() {
    let api = extraction("src/api.c", API_C);

    assert_eq!(api.coverage, FileCoverage::Full);
    assert_eq!(
        node_ids(&api),
        vec![
            "src/api.c",
            "src/api.c#function:add",
            "src/api.c#function:twice",
            "src/api.c#type:Pair",
            "src/api.c#type:mode",
        ]
    );
}

#[test]
fn a_function_span_covers_its_body() {
    let api = extraction("src/api.c", API_C);
    let twice = api
        .nodes
        .iter()
        .find(|n| n.id == "src/api.c#function:twice")
        .unwrap();

    assert_eq!(twice.signature, "int twice(int value) {");
    assert_eq!((twice.span.line_start, twice.span.line_end), (15, 18));
}

#[test]
fn includes_are_glob_bindings_and_system_headers_keep_their_bracket() {
    let api = extraction("src/api.c", API_C);

    let bindings: Vec<(&str, bool, Option<&str>)> = api
        .imports
        .iter()
        .map(|b| (b.path.as_str(), b.glob, b.local_name()))
        .collect();
    assert_eq!(
        bindings,
        vec![("<stdio.h>", true, None), ("api.h", true, None)]
    );

    let imports: Vec<&str> = api
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::Imports)
        .map(|e| e.symbol.as_str())
        .collect();
    assert_eq!(imports, vec!["<stdio.h>", "api.h"]);
}

#[test]
fn a_call_binds_to_its_definition_and_an_external_call_stays_unresolved() {
    let api = extraction("src/api.c", API_C);
    let mut calls: Vec<(&str, &str, EdgeProvenance)> = api
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::Calls)
        .map(|e| (e.from.as_str(), e.symbol.as_str(), e.provenance))
        .collect();
    calls.sort_unstable_by_key(|(_, symbol, _)| *symbol);

    assert_eq!(
        calls,
        vec![
            ("src/api.c#function:twice", "add", EdgeProvenance::LocalName),
            ("src/api.c#function:twice", "printf", EdgeProvenance::Syntax),
        ]
    );
}

#[test]
fn a_call_through_a_member_records_its_receiver_and_never_binds_by_name() {
    let dispatch = extraction("src/dispatch.c", DISPATCH_C);
    let call = dispatch
        .edges
        .iter()
        .find(|e| e.kind == SourceEdgeKind::Calls)
        .unwrap();

    assert_eq!(call.symbol, "apply");
    assert_eq!(call.receiver.as_deref(), Some("table"));
    assert_eq!(call.provenance, EdgeProvenance::Syntax);
    assert_eq!(call.from, "src/dispatch.c#function:run");
}

#[test]
fn a_macro_generated_function_is_a_gap_not_a_node() {
    let generated = extraction("src/macro_generated.c", MACRO_C);

    assert_eq!(generated.coverage, FileCoverage::Full);
    assert_eq!(
        node_ids(&generated),
        vec![
            "src/macro_generated.c",
            "src/macro_generated.c#function:declared"
        ]
    );
}

#[test]
fn both_branches_of_an_ifdef_are_distinct_nodes_of_one_symbol() {
    let branches = extraction("src/ifdef_branches.c", IFDEF_C);
    let compute: Vec<_> = branches
        .nodes
        .iter()
        .filter(|n| n.kind == SourceNodeKind::Function)
        .collect();

    assert_eq!(compute.len(), 2, "nodes: {:#?}", branches.nodes);
    assert_ne!(compute[0].id, compute[1].id);
    assert_eq!(
        compute[0].symbol_key,
        "src/ifdef_branches.c#function:compute"
    );
    assert_eq!(compute[1].symbol_key, compute[0].symbol_key);
    assert!(compute[0].id.starts_with(&compute[0].symbol_key));
}

#[test]
fn a_syntax_error_yields_a_parse_error_and_no_symbols() {
    let broken = extraction("src/syntax_error.c", SYNTAX_ERROR_C);

    assert!(
        matches!(broken.coverage, FileCoverage::ParseError { .. }),
        "coverage: {:?}",
        broken.coverage
    );
    assert_eq!(node_ids(&broken), vec!["src/syntax_error.c"]);
}

/// A prototype is a `References` edge, and the capability says so.
#[test]
fn the_references_capability_matches_the_edges_emitted() {
    let unit = extraction("src/a.c", "int area(int side);\n");

    assert!(unit
        .edges
        .iter()
        .any(|edge| edge.kind == SourceEdgeKind::References));
    assert!(CExtractor::new().capabilities().references);
}
