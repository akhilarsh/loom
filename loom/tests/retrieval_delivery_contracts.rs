//! Contracts for stage retrieval-delivery: intent routing and explained neighbours.
//!
//! Each test pins one rule of section 13 of
//! `doc/plans/briefs/source-graph-mechanism/design.md`, ranking an in-memory
//! graph built by the real extractors and resolver.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use loom::context::extract::{extract_file, registry};
use loom::context::graph_store::{FileEntry, ResolvedGraph};
use loom::context::rank::{EdgeDirection, NeighborVia, RankQuery, RankedCandidate};
use loom::context::rank_source::intent::{classify, QueryIntent};
use loom::context::rank_source::rank_source;
use loom::context::resolve_graph;
use loom::context::schema::{SelectionReason, TextSearchHint};
use loom::context::source_graph::SourceEdgeKind;
use loom::context::RetrievalConfig;

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

/// Ranks `text` against `graph` with no required ids, dependencies or paths.
fn ranked(graph: &ResolvedGraph, text: &str) -> Vec<RankedCandidate> {
    let query = RankQuery {
        text: text.to_string(),
        required_ids: Vec::new(),
        stage_dependency_ids: Vec::new(),
        dependency_paths: Vec::new(),
    };
    rank_source(&query, graph, &RetrievalConfig::default())
}

fn find<'a>(candidates: &'a [RankedCandidate], id: &str) -> Option<&'a RankedCandidate> {
    candidates.iter().find(|c| c.id.as_str() == id)
}

fn via_of<'a>(candidates: &'a [RankedCandidate], id: &str) -> &'a NeighborVia {
    let candidate =
        find(candidates, id).unwrap_or_else(|| panic!("no candidate {id} in {candidates:#?}"));
    assert!(
        candidate.reasons.contains(&SelectionReason::GraphNeighbor),
        "{id} must be a GraphNeighbor: {candidate:#?}"
    );
    candidate
        .via
        .as_ref()
        .unwrap_or_else(|| panic!("{id} carries no via: {candidate:#?}"))
}

#[test]
fn symbol_question_admits_a_one_word_name() {
    let graph = resolved(&[("src/lexer.rs", "pub fn tokenize() {}\n")]);

    let candidates = ranked(&graph, "what does tokenize do");

    let candidate = find(&candidates, "src/lexer.rs#function:tokenize")
        .unwrap_or_else(|| panic!("tokenize not admitted: {candidates:#?}"));
    assert!(
        candidate.reasons.contains(&SelectionReason::SymbolQuestion),
        "tokenize must carry SymbolQuestion: {candidate:#?}"
    );
}

#[test]
fn literal_query_is_classified_with_its_text() {
    let intent = classify("where do we log \"connection refused\"");

    assert_eq!(
        intent,
        QueryIntent::Literal {
            text: "connection refused".to_string(),
        }
    );
}

#[test]
fn literal_hint_escapes_single_quotes() {
    let hint = TextSearchHint::for_pattern("it's broken");

    assert_eq!(hint.command, r"rg -n -F -- 'it'\''s broken'");
    assert_eq!(hint.pattern, "it's broken");
}

#[test]
fn graph_neighbors_carry_their_edge() {
    let graph = resolved(&[(
        "src/lib.rs",
        "pub fn seed() {}\npub fn user() {\n    seed();\n}\n",
    )]);

    let candidates = ranked(&graph, "`seed`");

    let via = via_of(&candidates, "src/lib.rs#function:user");
    assert_eq!(via.seed, "src/lib.rs#function:seed", "via: {via:#?}");
    assert_eq!(via.edge_kind, SourceEdgeKind::Calls, "via: {via:#?}");
    assert_eq!(via.direction, EdgeDirection::Outgoing, "via: {via:#?}");
    assert_eq!(via.site_line, Some(3), "via: {via:#?}");
}

#[test]
fn relationship_query_seeds_direct_callers() {
    let graph = resolved(&[(
        "src/lib.rs",
        "pub fn target() {}\npub fn caller() {\n    target();\n}\npub fn bystander() {}\n",
    )]);

    let candidates = ranked(&graph, "who calls target");

    let via = via_of(&candidates, "src/lib.rs#function:caller");
    assert_eq!(via.seed, "src/lib.rs#function:target", "via: {via:#?}");
    assert!(
        find(&candidates, "src/lib.rs#function:bystander").is_none(),
        "bystander must not be admitted: {candidates:#?}"
    );
}

#[test]
fn prose_with_a_node_name_is_not_a_symbol_question() {
    let graph = resolved(&[("src/lexer.rs", "pub fn tokenize() {}\n")]);

    let candidates = ranked(&graph, "please tokenize the input before parsing");

    let symbol_questions: Vec<&RankedCandidate> = candidates
        .iter()
        .filter(|c| c.reasons.contains(&SelectionReason::SymbolQuestion))
        .collect();
    assert!(
        symbol_questions.is_empty(),
        "prose must not admit a SymbolQuestion: {symbol_questions:#?}"
    );
    assert_eq!(classify("what does the plan say"), QueryIntent::General);
}
