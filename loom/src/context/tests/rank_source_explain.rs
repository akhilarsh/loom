//! Neighbour explanations carried by ranked source candidates.

use super::source_fixtures::{full_node, graph, local_edge};
use crate::context::config::RetrievalConfig;
use crate::context::graph_store::ResolvedGraph;
use crate::context::pack::neighbor_explanation;
use crate::context::rank::{EdgeDirection, NeighborVia, RankQuery, RankedCandidate};
use crate::context::rank_source::{rank_source, MAX_EXPANDED_TOKENS};
use crate::context::schema::{estimate_tokens, SelectionReason, Span};
use crate::context::source_graph::{EdgeProvenance, SourceEdge, SourceEdgeKind};

const SEED: &str = "src/seed.rs#function:Seed";
const CALLEE: &str = "src/callee.rs#function:callee";

#[test]
fn seed_candidates_carry_no_via() {
    let mut fixture = graph(vec![
        (
            "src/seed.rs",
            vec![full_node(SEED, "src/seed.rs", &["Seed"], "fn Seed()")],
        ),
        (
            "src/callee.rs",
            vec![full_node(
                CALLEE,
                "src/callee.rs",
                &["callee"],
                "fn callee()",
            )],
        ),
    ]);
    fixture
        .files
        .get_mut("src/seed.rs")
        .expect("fixture file must exist")
        .edges = vec![local_edge(SEED, CALLEE, SourceEdgeKind::Calls, "callee")];
    let query = RankQuery {
        text: "inspect `Seed`".to_string(),
        ..RankQuery::default()
    };

    let ranked = rank_source(&query, &fixture, &RetrievalConfig::default());
    let seed = ranked
        .iter()
        .find(|candidate| candidate.id.as_str() == SEED)
        .expect("the backticked symbol must be ranked");

    assert_eq!(seed.via, None, "a seed is not explained by an edge");
}

const CALLER: &str = "src/caller.rs#function:caller";

/// `SEED` with one callee and one caller, each in its own file.
fn seed_with_caller_and_callee() -> ResolvedGraph {
    let mut fixture = graph(vec![
        (
            "src/seed.rs",
            vec![full_node(SEED, "src/seed.rs", &["Seed"], "fn Seed()")],
        ),
        (
            "src/callee.rs",
            vec![full_node(
                CALLEE,
                "src/callee.rs",
                &["callee"],
                "fn callee()",
            )],
        ),
        (
            "src/caller.rs",
            vec![full_node(
                CALLER,
                "src/caller.rs",
                &["caller"],
                "fn caller()",
            )],
        ),
    ]);
    add_edge(
        &mut fixture,
        "src/seed.rs",
        local_edge(SEED, CALLEE, SourceEdgeKind::Calls, "callee"),
        Some(12),
    );
    add_edge(
        &mut fixture,
        "src/caller.rs",
        local_edge(CALLER, SEED, SourceEdgeKind::Calls, "Seed"),
        Some(7),
    );
    fixture
}

/// Attach `edge` to `path`'s file, recording one reference site at `line`.
fn add_edge(fixture: &mut ResolvedGraph, path: &str, mut edge: SourceEdge, line: Option<usize>) {
    if let Some(line) = line {
        edge.sites = vec![Span {
            start_byte: 0,
            end_byte: 1,
            line_start: line,
            line_end: line,
        }];
    }
    fixture
        .files
        .get_mut(path)
        .expect("fixture file must exist")
        .edges
        .push(edge);
}

fn rank(fixture: &ResolvedGraph, text: &str) -> Vec<RankedCandidate> {
    let query = RankQuery {
        text: text.to_string(),
        ..RankQuery::default()
    };
    rank_source(&query, fixture, &RetrievalConfig::default())
}

fn via_of<'a>(ranked: &'a [RankedCandidate], id: &str) -> &'a NeighborVia {
    ranked
        .iter()
        .find(|candidate| candidate.id.as_str() == id)
        .unwrap_or_else(|| panic!("{id} must be ranked: {ranked:#?}"))
        .via
        .as_ref()
        .unwrap_or_else(|| panic!("{id} must carry a via"))
}

#[test]
fn callees_and_callers_carry_the_edge_that_admitted_them() {
    let ranked = rank(&seed_with_caller_and_callee(), "inspect `Seed`");

    let callee = via_of(&ranked, CALLEE);
    assert_eq!(callee.seed, SEED);
    assert_eq!(callee.edge_kind, SourceEdgeKind::Calls);
    assert_eq!(
        callee.direction,
        EdgeDirection::Incoming,
        "the seed calls it"
    );
    assert_eq!(callee.provenance, EdgeProvenance::LocalName);
    assert_eq!(callee.site_line, Some(12));

    let caller = via_of(&ranked, CALLER);
    assert_eq!(caller.seed, SEED);
    assert_eq!(
        caller.direction,
        EdgeDirection::Outgoing,
        "it calls the seed"
    );
    assert_eq!(caller.site_line, Some(7));
}

#[test]
fn an_edge_without_a_positioned_site_has_no_site_line() {
    let mut fixture = seed_with_caller_and_callee();
    fixture
        .files
        .get_mut("src/seed.rs")
        .expect("fixture file must exist")
        .edges
        .clear();
    add_edge(
        &mut fixture,
        "src/seed.rs",
        local_edge(SEED, CALLEE, SourceEdgeKind::Calls, "callee"),
        None,
    );

    let ranked = rank(&fixture, "inspect `Seed`");

    assert_eq!(via_of(&ranked, CALLEE).site_line, None);
}

fn via(kind: SourceEdgeKind, direction: EdgeDirection, site_line: Option<usize>) -> NeighborVia {
    NeighborVia {
        seed: SEED.to_string(),
        edge_kind: kind,
        direction,
        provenance: EdgeProvenance::LocalName,
        site_line,
    }
}

#[test]
fn explanation_names_the_seed_and_the_site_line() {
    let incoming = via(SourceEdgeKind::Calls, EdgeDirection::Incoming, Some(12));
    assert_eq!(
        neighbor_explanation(&incoming),
        "called by `Seed` at src/seed.rs:L12"
    );
    let outgoing = via(SourceEdgeKind::Calls, EdgeDirection::Outgoing, Some(12));
    assert_eq!(neighbor_explanation(&outgoing), "calls `Seed`");
}

#[test]
fn explanation_covers_references_and_omits_an_unknown_site() {
    let incoming = via(SourceEdgeKind::References, EdgeDirection::Incoming, Some(3));
    assert_eq!(
        neighbor_explanation(&incoming),
        "referenced by `Seed` at src/seed.rs:L3"
    );
    let unpositioned = via(SourceEdgeKind::References, EdgeDirection::Incoming, None);
    assert_eq!(neighbor_explanation(&unpositioned), "referenced by `Seed`");
    let outgoing = via(SourceEdgeKind::References, EdgeDirection::Outgoing, None);
    assert_eq!(neighbor_explanation(&outgoing), "references `Seed`");
}

/// Five seeds, each called by four functions with long names and signatures.
fn crowded_graph() -> ResolvedGraph {
    let mut files = Vec::new();
    let mut edges = Vec::new();
    for seed in 0..5 {
        let seed_id = format!("src/seed{seed}.rs#function:Seed{seed}");
        let seed_path = format!("src/seed{seed}.rs");
        let seed_name = format!("Seed{seed}");
        let seed_node = full_node(
            &seed_id,
            &seed_path,
            &[seed_name.as_str()],
            &format!("fn {seed_name}()"),
        );
        files.push((seed_path, vec![seed_node]));
        for caller in 0..4 {
            let path = format!("src/helper_{seed}_{caller}.rs");
            let name = format!("helper_with_a_long_descriptive_name_{seed}_{caller}");
            let id = format!("{path}#function:{name}");
            let signature = format!(
                "fn {name}(first_argument: SomeVeryLongTypeName, second_argument: AnotherLongTypeName)"
            );
            let node = full_node(&id, &path, &[name.as_str()], &signature);
            files.push((path.clone(), vec![node]));
            edges.push((
                path,
                local_edge(&id, &seed_id, SourceEdgeKind::Calls, "Seed"),
            ));
        }
    }
    let mut fixture = graph(files.iter().map(|(p, n)| (p.as_str(), n.clone())).collect());
    for (path, edge) in edges {
        add_edge(&mut fixture, &path, edge, Some(2));
    }
    fixture
}

#[test]
fn expansion_stops_at_the_token_cap() {
    let ranked = rank(
        &crowded_graph(),
        "inspect `Seed0` `Seed1` `Seed2` `Seed3` `Seed4`",
    );

    let neighbours: Vec<&RankedCandidate> = ranked
        .iter()
        .filter(|candidate| candidate.reasons.contains(&SelectionReason::GraphNeighbor))
        .collect();
    let spent: usize = neighbours
        .iter()
        .map(|candidate| {
            let via = candidate.via.as_ref().expect("a neighbour carries a via");
            candidate.token_count + estimate_tokens(&neighbor_explanation(via))
        })
        .sum();

    assert!(!neighbours.is_empty(), "some neighbours must be admitted");
    assert!(
        neighbours.len() < 12,
        "the token cap must bind before the count cap: {}",
        neighbours.len()
    );
    assert!(
        spent <= MAX_EXPANDED_TOKENS,
        "{spent} tokens admitted, cap {MAX_EXPANDED_TOKENS}"
    );
}
