//! The shared walk's evidence: reference sites, same-file binding, import
//! bindings and node identity, driven through the Rust grammar.

use std::path::Path;

use super::{run_query, QueryHarness};
use crate::context::extract::dialect::dialect_by_id;
use crate::context::extract::rust::RustExtractor;
use crate::context::extract::{ExtractorIdentity, FileExtraction, SourceGraphExtractor};
use crate::context::source_graph::{
    EdgeProvenance, ImportBinding, NodeLanguage, SourceEdge, SourceEdgeKind, SourceNodeKind, Span,
    MAX_CANDIDATES, UNRESOLVED_TARGET,
};

/// A Rust-grammar harness with a chosen query and receiver list, for the parts
/// of the capture protocol the Rust extractor's own query does not use yet.
struct TestHarness {
    query: &'static str,
    receivers: &'static [&'static str],
}

impl QueryHarness for TestHarness {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn query_source(&self) -> &'static str {
        self.query
    }

    fn identity(&self) -> ExtractorIdentity {
        ExtractorIdentity {
            dialect: "rust",
            grammar_version: "test",
            query_digest: "sha256:test".to_string(),
            extractor_version: 0,
        }
    }

    fn node_language(&self) -> NodeLanguage {
        NodeLanguage::Rust
    }

    fn kind_for_capture(&self, suffix: &str) -> Option<SourceNodeKind> {
        match suffix {
            "function" => Some(SourceNodeKind::Function),
            "implementation" => Some(SourceNodeKind::Implementation),
            _ => None,
        }
    }

    /// Echoes the statement as the alias, so a test sees what arrived.
    fn import_bindings(&self, statement: &str, path: &str, site: Span) -> Vec<ImportBinding> {
        vec![ImportBinding {
            path: path.to_string(),
            name: None,
            alias: Some(statement.to_string()),
            glob: false,
            exported_as: None,
            site,
        }]
    }

    fn self_receivers(&self) -> &'static [&'static str] {
        self.receivers
    }
}

/// Member calls with their receiver.
const RECEIVER_QUERY: &str = r#"
(impl_item type: (type_identifier) @name) @definition.implementation
(function_item name: (identifier) @name) @definition.function
(call_expression
  function: (field_expression
    value: (_) @call.receiver
    field: (field_identifier) @call.name))
"#;

/// The Rust grammar writes no qualifier on a definition, so this query
/// borrows a free function's return type as one: `fn run() -> W` stands in
/// for C++ `void W::run()`. Methods (with a `self` parameter) stay unqualified.
const QUALIFIER_QUERY: &str = r#"
(impl_item type: (type_identifier) @name) @definition.implementation
(function_item
  name: (identifier) @name
  parameters: (parameters (self_parameter))) @definition.function
(function_item
  name: (identifier) @name
  return_type: (type_identifier) @definition.qualifier) @definition.function
(call_expression
  function: (field_expression
    value: (_) @call.receiver
    field: (field_identifier) @call.name))
"#;

/// An import statement captured whole alongside its path.
const IMPORT_QUERY: &str = "(use_declaration argument: (_) @import.path) @import.statement";

/// An import path captured with no statement, which gets the default binding.
const PATH_ONLY_IMPORT_QUERY: &str = "(use_declaration argument: (_) @import.path)";

pub(super) fn rust(source: &str) -> FileExtraction {
    RustExtractor::new()
        .extract(Path::new("src/lib.rs"), source.as_bytes())
        .unwrap()
}

fn harnessed(
    query: &'static str,
    receivers: &'static [&'static str],
    source: &str,
) -> FileExtraction {
    let harness = TestHarness { query, receivers };
    run_query(&harness, Path::new("src/lib.rs"), source.as_bytes()).unwrap()
}

pub(super) fn function(scope: &str) -> String {
    format!("src/lib.rs#function:{scope}")
}

/// The single `Calls` edge leaving `from` for `symbol`.
fn call<'a>(extraction: &'a FileExtraction, from: &str, symbol: &str) -> &'a SourceEdge {
    let edges: Vec<_> = extraction
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::Calls && e.from == from && e.symbol == symbol)
        .collect();
    assert_eq!(
        edges.len(),
        1,
        "calls {from} -> {symbol}: {:#?}",
        extraction.edges
    );
    edges[0]
}

fn assert_bound(edge: &SourceEdge, to: &str, provenance: EdgeProvenance, confidence: f32) {
    assert_eq!(edge.to, to, "edge: {edge:#?}");
    assert_eq!(edge.provenance, provenance, "edge: {edge:#?}");
    assert!(
        (edge.confidence - confidence).abs() < 1e-6,
        "edge: {edge:#?}"
    );
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}

fn assert_unbound(edge: &SourceEdge, candidates: &[String]) {
    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert_eq!(edge.provenance, EdgeProvenance::Syntax, "edge: {edge:#?}");
    assert!(edge.confidence <= 0.5, "edge: {edge:#?}");
    assert_eq!(edge.candidates, candidates, "edge: {edge:#?}");
}

#[test]
fn repeated_calls_merge_into_one_edge_with_every_site() {
    let extraction = rust("fn helper() {}\nfn run() {\n    helper();\n    helper();\n}\n");

    let edge = call(&extraction, &function("run"), "helper");

    assert_bound(edge, &function("helper"), EdgeProvenance::LocalName, 0.8);
    let lines: Vec<usize> = edge.sites.iter().map(|site| site.line_start).collect();
    assert_eq!(lines, [3, 4], "edge: {edge:#?}");
}

#[test]
fn a_name_two_sibling_modules_define_keeps_both_candidates() {
    let extraction = rust(
        "mod a { pub fn helper() {} }\nmod b { pub fn helper() {} }\nfn run() { helper(); }\n",
    );

    let edge = call(&extraction, &function("run"), "helper");

    assert_unbound(edge, &[function("a::helper"), function("b::helper")]);
    for edge in &extraction.edges {
        if edge.confidence >= 1.0 {
            assert_eq!(edge.kind, SourceEdgeKind::Contains, "edge: {edge:#?}");
        }
    }
}

#[test]
fn more_candidates_than_the_cap_leave_the_list_empty() {
    let modules: String = (0..=MAX_CANDIDATES)
        .map(|n| format!("mod m{n} {{ pub fn helper() {{}} }}\n"))
        .collect();
    let extraction = rust(&format!("{modules}fn run() {{ helper(); }}\n"));

    assert_unbound(call(&extraction, &function("run"), "helper"), &[]);
}

#[test]
fn the_innermost_lexical_scope_binds() {
    let extraction = rust("fn outer() { fn helper() {} helper(); }\nfn helper() {}\n");

    let edge = call(&extraction, &function("outer"), "helper");

    assert_bound(
        edge,
        &function("outer::helper"),
        EdgeProvenance::LocalName,
        0.8,
    );
}

#[test]
fn a_name_out_of_lexical_scope_is_a_candidate_never_a_binding() {
    let hidden = rust("mod hidden {\n    pub fn helper() {}\n}\nfn run() {\n    helper();\n}\n");
    assert_unbound(
        call(&hidden, &function("run"), "helper"),
        &[function("hidden::helper")],
    );

    let lexical =
        rust("mod m {\n    pub fn helper() {}\n    pub fn run() {\n        helper();\n    }\n}\n");
    let edge = call(&lexical, &function("m::run"), "helper");
    assert_bound(edge, &function("m::helper"), EdgeProvenance::LocalName, 0.8);
}

#[test]
fn a_bare_call_never_reaches_a_member_in_rust() {
    let bare = rust("impl W { fn parse(&self) {} fn load(&self) { parse(); } }\n");
    assert_unbound(call(&bare, &function("W::load"), "parse"), &[]);

    let qualified =
        rust("struct W;\nimpl W { fn parse(&self) {} fn load(&self) { W::parse(); } }\n");
    let edge = call(&qualified, &function("W::load"), "W::parse");
    assert_bound(edge, &function("W::parse"), EdgeProvenance::LocalName, 0.8);
}

#[test]
fn a_qualified_call_from_a_free_function_binds_the_member() {
    let extraction = rust("struct W;\nimpl W { fn parse(&self) {} }\nfn run() { W::parse(); }\n");

    let edge = call(&extraction, &function("run"), "W::parse");

    assert_bound(edge, &function("W::parse"), EdgeProvenance::LocalName, 0.8);
}

#[test]
fn a_self_call_binds_to_the_member_of_the_enclosing_impl() {
    let source = "impl W { fn a(&self) { self.b(); } fn b(&self) {} }\n";
    let extraction = harnessed(RECEIVER_QUERY, &["self"], source);

    let edge = call(&extraction, &function("W::a"), "b");

    assert_bound(edge, &function("W::b"), EdgeProvenance::Receiver, 0.85);
    assert_eq!(edge.receiver.as_deref(), Some("self"), "edge: {edge:#?}");
}

#[test]
fn a_call_on_another_receiver_stays_unbound() {
    let extraction = harnessed(
        RECEIVER_QUERY,
        &["self"],
        "fn b() {}\nfn run() { obj.b(); }\n",
    );

    let edge = call(&extraction, &function("run"), "b");

    assert_unbound(edge, &[]);
    assert_eq!(edge.receiver.as_deref(), Some("obj"), "edge: {edge:#?}");
}

#[test]
fn a_qualified_function_scopes_itself_and_names_its_receiver_type() {
    let source = "impl W { fn m(&self) {} }\nfn run() -> W { this.m(); W }\n";
    let extraction = harnessed(QUALIFIER_QUERY, &["this"], source);

    let run = extraction
        .nodes
        .iter()
        .find(|node| node.id == function("W::run"))
        .unwrap_or_else(|| panic!("no qualified run: {:#?}", extraction.nodes));
    assert_eq!(run.scope, ["W", "run"]);
    let edge = call(&extraction, &function("W::run"), "m");
    assert_bound(edge, &function("W::m"), EdgeProvenance::Receiver, 0.85);
}

#[test]
fn an_import_with_only_a_path_yields_one_default_binding() {
    let extraction = harnessed(PATH_ONLY_IMPORT_QUERY, &[], "use a::b;\n");

    assert_eq!(extraction.imports.len(), 1, "{:#?}", extraction.imports);
    let binding = &extraction.imports[0];
    assert_eq!(
        (
            binding.path.as_str(),
            binding.name.as_deref(),
            binding.alias.as_deref(),
            binding.glob
        ),
        ("a::b", None, None, false)
    );
    assert_eq!(binding.site.start_byte, 4);
    let edge = extraction
        .edges
        .iter()
        .find(|e| e.kind == SourceEdgeKind::Imports)
        .expect("an Imports edge");
    assert_eq!(edge.sites, [binding.site], "edge: {edge:#?}");
}

#[test]
fn an_import_statement_reaches_the_harness_with_its_path() {
    let extraction = harnessed(IMPORT_QUERY, &[], "use a::b as c;\n");

    assert_eq!(extraction.imports.len(), 1, "{:#?}", extraction.imports);
    let binding = &extraction.imports[0];
    assert_eq!(binding.path, "a::b as c");
    assert_eq!(binding.alias.as_deref(), Some("use a::b as c;"));
}

#[test]
fn self_receivers_default_to_the_dialect_table() {
    let rust_dialect = dialect_by_id("rust").expect("the rust dialect");

    assert_eq!(
        RustExtractor::new().self_receivers(),
        rust_dialect.self_receivers
    );
}

/// A query compiles once and every later run shares it; a query that fails
/// to compile is never cached, so every run reports it.
#[test]
fn a_compiled_query_is_shared_and_a_bad_one_errors_on_every_run() {
    let harness = TestHarness {
        query: RECEIVER_QUERY,
        receivers: &[],
    };
    let language = harness.node_language();
    let first = super::compiled_query(&harness, &language).unwrap();
    let second = super::compiled_query(&harness, &language).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));

    let bad = TestHarness {
        query: "(no_such_node) @name",
        receivers: &[],
    };
    for _ in 0..2 {
        let error = run_query(&bad, Path::new("src/lib.rs"), b"fn f() {}\n").unwrap_err();
        assert!(
            error.to_string().contains("invalid tree-sitter query"),
            "{error:#}"
        );
    }
}
