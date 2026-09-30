//! Bounds on what intent routing admits: how many nodes one symbol question
//! may name, and how many neighbours and tokens of neighbours one relationship
//! query may add.

use super::{
    find, full_node, function, graph, graph_with_edges, ids, local_edge, ranked, CALLER, TARGET,
};
use crate::context::graph_store::ResolvedGraph;
use crate::context::rank::neighbor_explanation;
use crate::context::rank_source::MAX_EXPANDED_TOKENS;
use crate::context::schema::{
    estimate_tokens, FileCoverage, SelectionReason, SourceEdgeKind, SourceNodeKind,
};
use crate::context::tests::source_fixtures::node;

const HELPER: &str = "src/lib.rs#function:helper";

#[test]
fn a_symbol_question_naming_many_nodes_admits_nothing() {
    let files: Vec<(String, String)> = (0..4)
        .map(|n| (format!("src/m{n}.rs"), format!("src/m{n}.rs#function:new")))
        .collect();
    let fixture = graph(
        files
            .iter()
            .map(|(path, id)| (path.as_str(), vec![function(id, path, "new")]))
            .collect(),
    );

    let ranked = ranked("what is new", &fixture);

    assert!(ranked.is_empty(), "an ambiguous name admitted {ranked:#?}");
}

#[test]
fn a_symbol_question_naming_two_nodes_admits_both() {
    let (first, second) = ("src/a.rs#function:tokenize", "src/b.rs#function:tokenize");
    let fixture = graph(vec![
        ("src/a.rs", vec![function(first, "src/a.rs", "tokenize")]),
        ("src/b.rs", vec![function(second, "src/b.rs", "tokenize")]),
    ]);

    let ranked = ranked("what does tokenize do", &fixture);

    assert_eq!(ids(&ranked), vec![first, second]);
    assert!(ranked
        .iter()
        .all(|candidate| candidate.reasons == vec![SelectionReason::SymbolQuestion]));
}

#[test]
fn a_pronoun_symbol_question_admits_nothing() {
    let fixture = graph(vec![(
        "src/lib.rs",
        vec![function("src/lib.rs#function:it", "src/lib.rs", "it")],
    )]);

    for text in ["how does it work", "what does this do"] {
        let ranked = ranked(text, &fixture);
        assert!(ranked.is_empty(), "{text:?} admitted {ranked:#?}");
    }
}

/// `target` has ten callers and one callee, every one with a long signature.
fn heavy_graph() -> ResolvedGraph {
    let long = "x".repeat(80);
    let mut nodes = vec![
        function(TARGET, "src/lib.rs", "target"),
        full_node(
            HELPER,
            "src/lib.rs",
            &["helper"],
            &format!("fn helper_{}()", "y".repeat(400)),
        ),
    ];
    let mut edges = vec![local_edge(TARGET, HELPER, SourceEdgeKind::Calls, "helper")];
    for n in 0..10 {
        let id = format!("src/lib.rs#function:caller_{n}");
        let name = format!("caller_{n}_{long}");
        nodes.push(full_node(
            &id,
            "src/lib.rs",
            &[&name],
            &format!("fn {name}()"),
        ));
        edges.push(local_edge(&id, TARGET, SourceEdgeKind::Calls, "target"));
    }
    graph_with_edges("src/lib.rs", nodes, edges)
}

#[test]
fn routed_and_expanded_neighbours_share_one_token_budget() {
    let ranked = ranked("who calls target", &heavy_graph());

    let neighbours: Vec<_> = ranked
        .iter()
        .filter(|candidate| candidate.reasons.contains(&SelectionReason::GraphNeighbor))
        .collect();
    let total: usize = neighbours
        .iter()
        .map(|candidate| {
            let via = candidate
                .via
                .as_ref()
                .expect("a neighbour carries its edge");
            candidate.token_count + estimate_tokens(&neighbor_explanation(via))
        })
        .sum();
    assert!(!neighbours.is_empty(), "{ranked:#?}");
    assert!(
        neighbours.len() < 10,
        "the budget must stop the callers: {}",
        neighbours.len()
    );
    assert!(
        total <= MAX_EXPANDED_TOKENS,
        "{total} estimated tokens of neighbours past {MAX_EXPANDED_TOKENS}"
    );
    assert!(
        !ids(&ranked).contains(&HELPER),
        "expansion must start from what routing spent: {ranked:#?}"
    );
}

/// `target` has fourteen callers and one callee, every one with a short
/// signature: the whole lot fits the token budget, so only the count cap can
/// stop it.
fn crowded_graph() -> ResolvedGraph {
    let mut nodes = vec![
        function(TARGET, "src/lib.rs", "target"),
        function(HELPER, "src/lib.rs", "helper"),
    ];
    let mut edges = vec![local_edge(TARGET, HELPER, SourceEdgeKind::Calls, "helper")];
    for n in 0..14 {
        let (id, name) = (format!("src/lib.rs#function:c{n}"), format!("c{n}"));
        nodes.push(function(&id, "src/lib.rs", &name));
        edges.push(local_edge(&id, TARGET, SourceEdgeKind::Calls, "target"));
    }
    graph_with_edges("src/lib.rs", nodes, edges)
}

#[test]
fn routed_neighbours_count_against_the_expansion_cap() {
    let ranked = ranked("who calls target", &crowded_graph());

    let neighbours = ranked
        .iter()
        .filter(|candidate| candidate.reasons.contains(&SelectionReason::GraphNeighbor))
        .count();
    // `MAX_EXPANDED`: routing fills the cap, so expansion adds nothing.
    assert_eq!(neighbours, 12, "{ranked:#?}");
    assert!(
        !ids(&ranked).contains(&HELPER),
        "expansion must start from what routing admitted: {ranked:#?}"
    );
}

#[test]
fn an_impact_query_never_admits_a_file_node() {
    let file = "src/lib.rs";
    let file_node = node(
        file,
        file,
        &[],
        "",
        SourceNodeKind::File,
        FileCoverage::Full,
    );
    let fixture = graph_with_edges(
        "src/lib.rs",
        vec![
            function(TARGET, "src/lib.rs", "target"),
            function(CALLER, "src/lib.rs", "caller"),
            file_node,
        ],
        vec![
            local_edge(CALLER, TARGET, SourceEdgeKind::Calls, "target"),
            local_edge(file, TARGET, SourceEdgeKind::Calls, "target"),
        ],
    );

    let ranked = ranked("impact of changing target", &fixture);

    assert!(ids(&ranked).contains(&CALLER), "{ranked:#?}");
    assert!(!ids(&ranked).contains(&file), "{ranked:#?}");
}

/// `count` functions named `new`, each called by its own `caller<n>`.
fn shared_name_graph(count: usize) -> ResolvedGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for n in 0..count {
        let (new, caller) = (
            format!("src/lib.rs#function:new{n}"),
            format!("src/lib.rs#function:caller{n}"),
        );
        nodes.push(function(&new, "src/lib.rs", "new"));
        nodes.push(function(&caller, "src/lib.rs", &format!("caller{n}")));
        edges.push(local_edge(&caller, &new, SourceEdgeKind::Calls, "new"));
    }
    graph_with_edges("src/lib.rs", nodes, edges)
}

#[test]
fn a_relationship_naming_many_nodes_seeds_nothing() {
    let ranked = ranked("who calls new", &shared_name_graph(4));

    for candidate in &ranked {
        assert!(
            !candidate.reasons.contains(&SelectionReason::ExactSymbol)
                && !candidate.reasons.contains(&SelectionReason::GraphNeighbor),
            "an ambiguous name seeded {candidate:#?}"
        );
    }
}

#[test]
fn a_relationship_naming_three_nodes_seeds_each() {
    let ranked = ranked("who calls new", &shared_name_graph(3));

    for n in 0..3 {
        let seed = find(&ranked, &format!("src/lib.rs#function:new{n}"));
        assert!(seed.reasons.contains(&SelectionReason::ExactSymbol));
        let caller = find(&ranked, &format!("src/lib.rs#function:caller{n}"));
        assert!(caller.reasons.contains(&SelectionReason::GraphNeighbor));
    }
}
