//! Bounds on what intent routing admits: how many nodes one symbol question
//! may name, and how many tokens of neighbours one relationship query may add.

use super::{full_node, function, graph, graph_with_edges, ids, local_edge, ranked, TARGET};
use crate::context::graph_store::ResolvedGraph;
use crate::context::pack::neighbor_explanation;
use crate::context::rank_source::MAX_EXPANDED_TOKENS;
use crate::context::schema::{estimate_tokens, SelectionReason, SourceEdgeKind};

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
