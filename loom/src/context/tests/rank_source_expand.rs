use super::source_fixtures::{
    full_node, graph, local_edge, node, source_candidate, unresolved_edge,
};
use crate::context::config::RetrievalConfig;
use crate::context::graph_store::ResolvedGraph;
use crate::context::rank::{RankQuery, RankedCandidate};
use crate::context::rank_source::{expand_from_seeds_for_test, rank_source};
use crate::context::schema::{
    Channel, Confidence, FileCoverage, SelectionReason, SourceNode, SourceNodeKind,
};
use crate::context::source_graph::{
    EdgeProvenance, SourceEdge, SourceEdgeKind, Span, UNRESOLVED_TARGET,
};

const SEED: &str = "src/seed.rs#function:Seed";
const NEIGHBOUR: &str = "src/neighbour.rs#function:neighbour";

fn function(id: &str, path: &str) -> SourceNode {
    full_node(id, path, &["fixture"], "fn fixture()")
}

fn candidate(id: &str, score: f32, reason: SelectionReason) -> RankedCandidate {
    let mut candidate = source_candidate(id, score, 1);
    candidate.reasons = vec![reason];
    candidate
}

fn graph_with_edges(nodes: Vec<SourceNode>, edges: Vec<SourceEdge>) -> ResolvedGraph {
    let mut graph = graph(vec![("src/fixture.rs", nodes)]);
    graph
        .files
        .get_mut("src/fixture.rs")
        .expect("fixture file must exist")
        .edges = edges;
    graph
}

fn expand(ranked: Vec<RankedCandidate>, graph: &ResolvedGraph) -> Vec<RankedCandidate> {
    expand_from_seeds_for_test(ranked, graph, &RetrievalConfig::default())
}

fn resolved_edge(from: &str, to: &str, kind: SourceEdgeKind, confidence: f32) -> SourceEdge {
    let mut edge = SourceEdge::bound(
        from,
        to,
        kind,
        to,
        Span::default(),
        EdgeProvenance::UniqueName,
    );
    edge.confidence = confidence;
    edge
}

#[test]
fn an_exact_symbol_seed_pulls_in_its_resolved_callers_and_callees_as_graph_neighbours() {
    let caller = "src/caller.rs#function:caller";
    let callee = "src/callee.rs#function:callee";
    let fixture = graph_with_edges(
        vec![
            full_node(SEED, "src/seed.rs", &["Seed"], "fn Seed()"),
            function(caller, "src/caller.rs"),
            function(callee, "src/callee.rs"),
        ],
        vec![
            local_edge(SEED, callee, SourceEdgeKind::Calls, "callee"),
            local_edge(caller, SEED, SourceEdgeKind::Calls, "Seed"),
        ],
    );
    let query = RankQuery {
        text: "inspect `Seed`".to_string(),
        ..RankQuery::default()
    };

    let ranked = rank_source(&query, &fixture, &RetrievalConfig::default());
    let ids: Vec<&str> = ranked.iter().map(|item| item.id.as_str()).collect();

    assert_eq!(ids, vec![SEED, callee, caller]);
}

#[test]
fn contains_and_imports_edges_never_expand() {
    let contained = "src/child.rs#function:child";
    let imported = "src/import.rs#function:imported";
    let fixture = graph_with_edges(
        vec![
            function(SEED, "src/seed.rs"),
            function(contained, "src/child.rs"),
            function(imported, "src/import.rs"),
        ],
        vec![
            local_edge(SEED, contained, SourceEdgeKind::Contains, "child"),
            local_edge(SEED, imported, SourceEdgeKind::Imports, "imported"),
        ],
    );

    let ranked = vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)];
    assert_eq!(expand(ranked.clone(), &fixture), ranked);
}

#[test]
fn unresolved_and_low_confidence_edges_never_expand() {
    let low = "src/low.rs#function:low";
    let fixture = graph_with_edges(
        vec![
            function(SEED, "src/seed.rs"),
            function(low, "src/low.rs"),
            function(UNRESOLVED_TARGET, "src/unresolved.rs"),
        ],
        vec![
            unresolved_edge(SEED, SourceEdgeKind::Calls, "missing"),
            resolved_edge(SEED, low, SourceEdgeKind::Calls, 0.49),
        ],
    );

    let ranked = vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)];
    assert_eq!(expand(ranked.clone(), &fixture), ranked);
}

#[test]
fn a_file_node_is_never_a_neighbour() {
    let file = node(
        "src/file.rs",
        "src/file.rs",
        &[],
        "",
        SourceNodeKind::File,
        FileCoverage::Full,
    );
    let fixture = graph_with_edges(
        vec![function(SEED, "src/seed.rs"), file],
        vec![local_edge(
            SEED,
            "src/file.rs",
            SourceEdgeKind::References,
            "file",
        )],
    );

    let ranked = vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)];
    assert_eq!(expand(ranked.clone(), &fixture), ranked);
}

#[test]
fn a_neighbour_that_is_already_a_candidate_is_not_duplicated() {
    let fixture = graph_with_edges(
        vec![
            function(SEED, "src/seed.rs"),
            function(NEIGHBOUR, "src/neighbour.rs"),
        ],
        vec![local_edge(
            SEED,
            NEIGHBOUR,
            SourceEdgeKind::Calls,
            "neighbour",
        )],
    );
    let ranked = vec![
        candidate(SEED, 10.0, SelectionReason::ExactSymbol),
        candidate(NEIGHBOUR, 1.0, SelectionReason::Lexical),
    ];

    let expanded = expand(ranked.clone(), &fixture);

    assert_eq!(expanded, ranked);
}

#[test]
fn expansion_is_capped_per_seed_and_overall() {
    let neighbours = [
        ("n-a", 0.9),
        ("n-b", 0.9),
        ("n-c", 0.8),
        ("n-d", 0.7),
        ("n-e", 0.6),
        ("n-f", 0.5),
    ];
    let mut nodes = vec![function(SEED, "src/seed.rs")];
    let mut edges = Vec::new();
    for (id, confidence) in neighbours {
        nodes.push(function(id, &format!("src/{id}.rs")));
        edges.push(resolved_edge(id, SEED, SourceEdgeKind::Calls, confidence));
    }
    let fixture = graph_with_edges(nodes, edges);
    let expanded = expand(
        vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)],
        &fixture,
    );
    let ids: Vec<&str> = expanded[1..].iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, vec!["n-a", "n-b", "n-c", "n-d"]);

    let mut ranked = Vec::new();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for seed in 0..5 {
        let seed_id = format!("seed-{seed}");
        nodes.push(function(&seed_id, &format!("src/{seed_id}.rs")));
        ranked.push(candidate(
            &seed_id,
            100.0 - seed as f32,
            SelectionReason::ExactSymbol,
        ));
        for neighbour in 0..4 {
            let id = format!("n-{seed}-{neighbour}");
            nodes.push(function(&id, &format!("src/{id}.rs")));
            edges.push(local_edge(&seed_id, &id, SourceEdgeKind::Calls, &id));
        }
    }
    let expanded = expand(ranked, &graph_with_edges(nodes, edges));
    assert_eq!(expanded.len(), 17, "five seeds plus twelve neighbours");
    assert_eq!(expanded[16].id.as_str(), "n-2-3");
}

#[test]
fn no_seed_means_no_expansion_and_no_adjacency_work() {
    let fixture = graph_with_edges(
        vec![
            function(SEED, "src/seed.rs"),
            function(NEIGHBOUR, "src/neighbour.rs"),
        ],
        vec![local_edge(
            SEED,
            NEIGHBOUR,
            SourceEdgeKind::Calls,
            "neighbour",
        )],
    );
    let ranked = vec![candidate(SEED, 1.0, SelectionReason::Lexical)];

    assert_eq!(expand(ranked.clone(), &fixture), ranked);
}

#[test]
fn a_neighbour_scores_below_its_seed_and_carries_only_the_graph_neighbour_reason() {
    let fixture = graph_with_edges(
        vec![
            function(SEED, "src/seed.rs"),
            function(NEIGHBOUR, "src/neighbour.rs"),
        ],
        vec![local_edge(
            SEED,
            NEIGHBOUR,
            SourceEdgeKind::Implements,
            "neighbour",
        )],
    );

    let expanded = expand(
        vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)],
        &fixture,
    );
    let neighbour = &expanded[1];

    assert!(neighbour.score < expanded[0].score);
    assert!((neighbour.score - 2.0).abs() < 1e-6);
    assert_eq!(neighbour.channel, Channel::Source);
    assert_eq!(neighbour.reasons, vec![SelectionReason::GraphNeighbor]);
    assert_eq!(neighbour.matched_term_count, 0);
    assert_eq!(neighbour.confidence_ceiling, Some(Confidence::Medium));
}

#[test]
fn a_neighbour_in_a_test_path_is_downweighted_like_any_other_node() {
    let test_neighbour = "tests/neighbour.rs#function:neighbour";
    let fixture = graph_with_edges(
        vec![
            function(SEED, "src/seed.rs"),
            function(test_neighbour, "tests/neighbour.rs"),
        ],
        vec![local_edge(
            SEED,
            test_neighbour,
            SourceEdgeKind::Extends,
            "neighbour",
        )],
    );

    let expanded = expand(
        vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)],
        &fixture,
    );
    let expected = 10.0 * 0.2 * RetrievalConfig::default().test_path_factor;

    assert!((expanded[1].score - expected).abs() < 1e-6);
}

#[test]
fn expansion_output_is_deterministic_across_two_runs() {
    let fixture = graph_with_edges(
        vec![
            function(SEED, "src/seed.rs"),
            function("b", "src/b.rs"),
            function("a", "src/a.rs"),
        ],
        vec![
            resolved_edge(SEED, "b", SourceEdgeKind::References, 0.8),
            resolved_edge(SEED, "a", SourceEdgeKind::References, 0.8),
        ],
    );
    let ranked = vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)];

    assert_eq!(expand(ranked.clone(), &fixture), expand(ranked, &fixture));
}

#[test]
fn a_neighbour_without_full_coverage_never_expands() {
    let partial = node(
        NEIGHBOUR,
        "src/neighbour.rs",
        &["neighbour"],
        "fn neighbour()",
        SourceNodeKind::Function,
        FileCoverage::Partial {
            detail: "fixture is partial".to_string(),
        },
    );
    let fixture = graph_with_edges(
        vec![function(SEED, "src/seed.rs"), partial],
        vec![local_edge(
            SEED,
            NEIGHBOUR,
            SourceEdgeKind::Calls,
            "neighbour",
        )],
    );
    let ranked = vec![candidate(SEED, 10.0, SelectionReason::ExactSymbol)];

    assert_eq!(expand(ranked.clone(), &fixture), ranked);
}

#[path = "rank_source_expand_fusion.rs"]
mod fusion_tests;
