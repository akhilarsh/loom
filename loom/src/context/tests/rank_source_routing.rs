//! Intent routing inside [`crate::context::rank_source::rank_source`].

mod caps;

use super::source_fixtures::{full_node, graph, local_edge};
use crate::context::config::RetrievalConfig;
use crate::context::graph_store::ResolvedGraph;
use crate::context::rank::{EdgeDirection, RankQuery, RankedCandidate};
use crate::context::rank_source::rank_source;
use crate::context::schema::{Confidence, SelectionReason, SourceNode};
use crate::context::source_graph::{EdgeProvenance, SourceEdge, SourceEdgeKind, Span};

const TARGET: &str = "src/lib.rs#function:target";
const CALLER: &str = "src/lib.rs#function:caller";
const HELPER: &str = "src/lib.rs#function:helper";
const BYSTANDER: &str = "src/lib.rs#function:bystander";

fn ranked(text: &str, fixture: &ResolvedGraph) -> Vec<RankedCandidate> {
    let query = RankQuery {
        text: text.to_string(),
        ..RankQuery::default()
    };
    rank_source(&query, fixture, &RetrievalConfig::default())
}

fn find<'a>(ranked: &'a [RankedCandidate], id: &str) -> &'a RankedCandidate {
    ranked
        .iter()
        .find(|candidate| candidate.id.as_str() == id)
        .unwrap_or_else(|| panic!("no candidate {id} in {ranked:#?}"))
}

fn ids(ranked: &[RankedCandidate]) -> Vec<&str> {
    ranked
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect()
}

fn function(id: &str, path: &str, name: &str) -> SourceNode {
    full_node(id, path, &[name], &format!("fn {name}()"))
}

fn graph_with_edges(path: &str, nodes: Vec<SourceNode>, edges: Vec<SourceEdge>) -> ResolvedGraph {
    let mut fixture = graph(vec![(path, nodes)]);
    fixture
        .files
        .get_mut(path)
        .expect("fixture file must exist")
        .edges = edges;
    fixture
}

/// `caller` calls `target` at line 7, `target` calls `helper`, and `bystander`
/// touches nothing.
fn call_chain() -> ResolvedGraph {
    let site = Span {
        line_start: 7,
        line_end: 7,
        ..Span::default()
    };
    graph_with_edges(
        "src/lib.rs",
        vec![
            function(TARGET, "src/lib.rs", "target"),
            function(CALLER, "src/lib.rs", "caller"),
            function(HELPER, "src/lib.rs", "helper"),
            function(BYSTANDER, "src/lib.rs", "bystander"),
        ],
        vec![
            SourceEdge::bound(
                CALLER,
                TARGET,
                SourceEdgeKind::Calls,
                "target",
                site,
                EdgeProvenance::LocalName,
            ),
            local_edge(TARGET, HELPER, SourceEdgeKind::Calls, "helper"),
        ],
    )
}

#[test]
fn exact_symbol_candidates_carry_no_via() {
    let fixture = graph(vec![(
        "src/seed.rs",
        vec![full_node(
            "src/seed.rs#function:Seed",
            "src/seed.rs",
            &["Seed"],
            "fn Seed()",
        )],
    )]);
    let query = RankQuery {
        text: "inspect `Seed`".to_string(),
        ..RankQuery::default()
    };

    let ranked = rank_source(&query, &fixture, &RetrievalConfig::default());

    assert!(!ranked.is_empty(), "the exact symbol must be admitted");
    assert!(
        ranked.iter().all(|candidate| candidate.via.is_none()),
        "an exact symbol is a seed, not a neighbour: {ranked:#?}"
    );
}

#[test]
fn a_symbol_question_admits_the_named_node_at_low_confidence() {
    let fixture = graph(vec![(
        "src/lexer.rs",
        vec![function(
            "src/lexer.rs#function:tokenize",
            "src/lexer.rs",
            "tokenize",
        )],
    )]);

    let ranked = ranked("what does tokenize do", &fixture);

    let candidate = find(&ranked, "src/lexer.rs#function:tokenize");
    assert_eq!(candidate.reasons, vec![SelectionReason::SymbolQuestion]);
    assert_eq!(candidate.confidence(), Confidence::Low);
    assert_eq!(candidate.confidence_ceiling, Some(Confidence::Low));
    assert_eq!(candidate.matched_term_count, 1);
    assert!(candidate.score > 0.0, "{candidate:#?}");
}

#[test]
fn a_symbol_question_naming_no_node_admits_nothing() {
    let fixture = graph(vec![(
        "src/lexer.rs",
        vec![function(
            "src/lexer.rs#function:tokenize",
            "src/lexer.rs",
            "tokenize",
        )],
    )]);

    for text in ["what does the parser do", "what does parser do"] {
        let ranked = ranked(text, &fixture);
        assert!(ranked.is_empty(), "{text:?} admitted {ranked:#?}");
    }
}

#[test]
fn a_qualified_symbol_question_admits_only_the_qualified_node() {
    let method = "src/lexer.rs#method:Lexer::tokenize";
    let fixture = graph(vec![
        (
            "src/lexer.rs",
            vec![full_node(
                method,
                "src/lexer.rs",
                &["Lexer", "tokenize"],
                "fn tokenize(&self)",
            )],
        ),
        (
            "src/free.rs",
            vec![function(
                "src/free.rs#function:tokenize",
                "src/free.rs",
                "tokenize",
            )],
        ),
    ]);

    let ranked = ranked("where is Lexer::tokenize defined", &fixture);

    assert_eq!(ids(&ranked), vec![method]);
    assert_eq!(ranked[0].reasons, vec![SelectionReason::SymbolQuestion]);
}

#[test]
fn a_symbol_question_keeps_test_path_demotion() {
    let source = "src/lexer.rs#function:tokenize";
    let test = "tests/lexer.rs#function:tokenize";
    let fixture = graph(vec![
        (
            "src/lexer.rs",
            vec![function(source, "src/lexer.rs", "tokenize")],
        ),
        (
            "tests/lexer.rs",
            vec![function(test, "tests/lexer.rs", "tokenize")],
        ),
    ]);

    let ranked = ranked("what does tokenize do", &fixture);

    let factor = RetrievalConfig::default().test_path_factor;
    let expected = find(&ranked, source).score * factor;
    let demoted = find(&ranked, test).score;
    assert!((demoted - expected).abs() < 1e-4, "{ranked:#?}");
}

#[test]
fn relationship_query_seeds_direct_neighbours() {
    let ranked = ranked("who calls target", &call_chain());

    assert!(
        find(&ranked, TARGET)
            .reasons
            .contains(&SelectionReason::ExactSymbol),
        "the named node is the seed: {ranked:#?}"
    );
    let caller = find(&ranked, CALLER);
    assert!(caller.reasons.contains(&SelectionReason::GraphNeighbor));
    let via = caller.via.as_ref().expect("a caller carries its edge");
    assert_eq!(via.seed, TARGET);
    assert_eq!(via.edge_kind, SourceEdgeKind::Calls);
    assert_eq!(via.direction, EdgeDirection::Outgoing);
    assert_eq!(via.provenance, EdgeProvenance::LocalName);
    assert_eq!(via.site_line, Some(7));
    assert!(!ids(&ranked).contains(&BYSTANDER), "{ranked:#?}");
}

#[test]
fn a_callees_query_marks_the_edge_incoming() {
    let ranked = ranked("what does target call", &call_chain());

    let via = find(&ranked, HELPER)
        .via
        .as_ref()
        .expect("a callee carries its edge");
    assert_eq!(via.seed, TARGET);
    assert_eq!(via.direction, EdgeDirection::Incoming);
}

#[test]
fn a_neighbour_already_ranked_merges_instead_of_duplicating() {
    let ranked = ranked("who calls target from `caller`", &call_chain());

    let copies = ids(&ranked).iter().filter(|id| **id == CALLER).count();
    assert_eq!(copies, 1, "{ranked:#?}");
    let caller = find(&ranked, CALLER);
    assert!(caller.reasons.contains(&SelectionReason::ExactSymbol));
    assert!(caller.reasons.contains(&SelectionReason::GraphNeighbor));
    assert_eq!(
        caller.via.as_ref().map(|via| via.seed.as_str()),
        Some(TARGET)
    );
}

#[test]
fn a_literal_query_halves_lexical_scores_and_keeps_the_exact_path_rung() {
    let exact = "src/store.rs#function:parse_tokens";
    let lexical = "src/other.rs#function:parse_tokens";
    let fixture = graph(vec![
        (
            "src/store.rs",
            vec![function(exact, "src/store.rs", "parse_tokens")],
        ),
        (
            "src/other.rs",
            vec![function(lexical, "src/other.rs", "parse_tokens")],
        ),
        (
            "src/misc.rs",
            vec![
                function("src/misc.rs#function:open", "src/misc.rs", "open"),
                function("src/misc.rs#function:close", "src/misc.rs", "close"),
                function("src/misc.rs#function:flush", "src/misc.rs", "flush"),
            ],
        ),
    ]);

    let general = ranked("parse tokens in src/store.rs", &fixture);
    let literal = ranked("\"parse tokens\" in src/store.rs", &fixture);

    let rung = 100.0;
    let (general_exact, literal_exact) = (find(&general, exact), find(&literal, exact));
    assert_eq!(
        general_exact.reasons,
        vec![SelectionReason::ExactPath, SelectionReason::Lexical]
    );
    assert_eq!(literal_exact.reasons, general_exact.reasons);
    assert!(general_exact.score > rung, "{general:#?}");
    let halved = (general_exact.score - rung) * 0.5;
    assert!(
        (literal_exact.score - rung - halved).abs() < 1e-4,
        "{literal:#?}"
    );
    let halved = find(&general, lexical).score * 0.5;
    assert!(
        (find(&literal, lexical).score - halved).abs() < 1e-4,
        "{literal:#?}"
    );
}

/// Snapshot of today's ranking for a general query: exact-path nodes first,
/// then the exact symbol, then its two neighbours in path order.
#[test]
fn a_general_query_ranks_as_it_did_before_routing() {
    let seed = "src/widgets.rs#function:Seed";
    let callee = "src/callee.rs#function:callee";
    let caller = "src/caller.rs#function:caller";
    let bystander = "src/other.rs#function:bystander";
    let mut open = function("src/store.rs#function:open", "src/store.rs", "open");
    open.span.line_start = 1;
    let mut close = function("src/store.rs#function:close", "src/store.rs", "close");
    close.span.line_start = 5;
    let mut fixture = graph(vec![
        ("src/store.rs", vec![open, close]),
        (
            "src/widgets.rs",
            vec![function(seed, "src/widgets.rs", "Seed")],
        ),
        (
            "src/callee.rs",
            vec![function(callee, "src/callee.rs", "callee")],
        ),
        (
            "src/caller.rs",
            vec![function(caller, "src/caller.rs", "caller")],
        ),
        (
            "src/other.rs",
            vec![function(bystander, "src/other.rs", "bystander")],
        ),
    ]);
    let edges = vec![
        local_edge(seed, callee, SourceEdgeKind::Calls, "callee"),
        local_edge(caller, seed, SourceEdgeKind::Calls, "Seed"),
    ];
    let widgets = fixture.files.get_mut("src/widgets.rs");
    widgets.expect("seed file must exist").edges = edges;

    let ranked = ranked("inspect `Seed` and src/store.rs", &fixture);

    assert_eq!(
        ids(&ranked),
        vec![
            "src/store.rs#function:open",
            "src/store.rs#function:close",
            seed,
            callee,
            caller,
        ]
    );
}
