//! Direct neighbours with call sites, candidate rows, and candidate-aware impact.

use super::fixtures::*;
use super::*;
use crate::context::source_graph::{
    SourceEdge, SourceEdgeKind, Span, AMBIGUOUS_CANDIDATE_CONFIDENCE, LOCAL_NAME_CONFIDENCE,
};

fn site(line: usize) -> Span {
    Span {
        start_byte: line * 10,
        end_byte: line * 10 + 5,
        line_start: line,
        line_end: line,
    }
}

fn bound(
    from: &str,
    to: &str,
    kind: SourceEdgeKind,
    provenance: EdgeProvenance,
    symbol: &str,
) -> SourceEdge {
    SourceEdge::bound(from, to, kind, symbol, site(3), provenance)
}

/// `run` calls a name that could mean either `alpha` or `beta`.
fn ambiguous_graph() -> ResolvedGraph {
    let run = func_id("src/run.rs", "run");
    let ambiguous = unresolved_edge(&run, SourceEdgeKind::Calls, "go").with_candidates(vec![
        func_id("src/alpha.rs", "alpha"),
        func_id("src/beta.rs", "beta"),
    ]);
    graph_from(vec![
        ("src/run.rs", &["run"], vec![ambiguous]),
        ("src/alpha.rs", &["alpha"], vec![]),
        ("src/beta.rs", &["beta"], vec![]),
    ])
}

fn hit_ids(result: &ImpactResult) -> Vec<&str> {
    result.hits.iter().map(|hit| hit.id.as_str()).collect()
}

#[test]
fn callers_include_a_candidate_row_with_its_set_size() {
    let graph = ambiguous_graph();

    let (callers, _) = direct_callers(&graph, &func_id("src/alpha.rs", "alpha"), 0);

    assert_eq!(callers.len(), 1);
    let row = &callers[0];
    assert_eq!(row.id, func_id("src/run.rs", "run"));
    assert_eq!(row.candidate_of, Some(2));
    assert_eq!(row.confidence, AMBIGUOUS_CANDIDATE_CONFIDENCE);
    assert_eq!(row.provenance, EdgeProvenance::Syntax);
    assert_eq!(row.symbol, "go");
    assert_eq!(row.site_path, "src/run.rs");
}

#[test]
fn callees_list_every_candidate_of_an_ambiguous_call() {
    let graph = ambiguous_graph();

    let (callees, _) = direct_callees(&graph, &func_id("src/run.rs", "run"), 0);

    let ids: Vec<&str> = callees.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["src/alpha.rs#function:alpha", "src/beta.rs#function:beta"]
    );
    assert!(callees.iter().all(|row| row.candidate_of == Some(2)));
}

#[test]
fn a_repeated_call_carries_both_sites_from_the_callers_file() {
    let caller = func_id("src/caller.rs", "caller");
    let callee = func_id("src/callee.rs", "callee");
    let mut edge = bound(
        &caller,
        &callee,
        SourceEdgeKind::Calls,
        EdgeProvenance::Import,
        "callee",
    );
    edge.sites = vec![site(3), site(9)];
    let graph = graph_from(vec![
        ("src/caller.rs", &["caller"], vec![edge]),
        ("src/callee.rs", &["callee"], vec![]),
    ]);

    let (callers, _) = direct_callers(&graph, &callee, 0);

    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].sites, vec![site(3), site(9)]);
    assert_eq!(callers[0].site_path, "src/caller.rs");
    assert_eq!(callers[0].path, "src/caller.rs");
    assert_eq!(callers[0].candidate_of, None);
    assert_eq!(callers[0].symbol, "callee");
}

#[test]
fn callers_exclude_reference_and_implement_edges() {
    let target = func_id("src/target.rs", "target");
    let caller = func_id("src/caller.rs", "caller");
    let graph = multi_edge_kind_graph();

    let (callers, _) = direct_callers(&graph, &target, 0);

    assert_eq!(
        callers
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec![caller.as_str()]
    );
}

#[test]
fn references_contain_only_reference_edges() {
    let target = func_id("src/target.rs", "target");
    let referrer = func_id("src/referrer.rs", "referrer");
    let graph = multi_edge_kind_graph();

    let (references, _) = direct_references(&graph, &target, 0);

    assert_eq!(
        references
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec![referrer.as_str()]
    );
    assert_eq!(references[0].edge_kind, SourceEdgeKind::References);
}

/// Graph with three different edge kinds from target: Calls, References, Implements.
fn multi_edge_kind_graph() -> ResolvedGraph {
    let target = func_id("src/target.rs", "target");
    let caller = func_id("src/caller.rs", "caller");
    let referrer = func_id("src/referrer.rs", "referrer");
    let implementer = func_id("src/implementer.rs", "implementer");
    graph_from(vec![
        (
            "src/caller.rs",
            &["caller"],
            vec![bound(
                &caller,
                &target,
                SourceEdgeKind::Calls,
                EdgeProvenance::LocalName,
                "target",
            )],
        ),
        (
            "src/referrer.rs",
            &["referrer"],
            vec![bound(
                &referrer,
                &target,
                SourceEdgeKind::References,
                EdgeProvenance::LocalName,
                "target",
            )],
        ),
        (
            "src/implementer.rs",
            &["implementer"],
            vec![bound(
                &implementer,
                &target,
                SourceEdgeKind::Implements,
                EdgeProvenance::LocalName,
                "target",
            )],
        ),
        ("src/target.rs", &["target"], vec![]),
    ])
}

/// `caller -> target` through a candidate, and `far -> caller` bound.
fn candidate_chain() -> ResolvedGraph {
    let caller = func_id("src/caller.rs", "caller");
    let far = func_id("src/far.rs", "far");
    let ambiguous = unresolved_edge(&caller, SourceEdgeKind::Calls, "go").with_candidates(vec![
        func_id("src/target.rs", "target"),
        func_id("src/other.rs", "other"),
    ]);
    graph_from(vec![
        ("src/caller.rs", &["caller"], vec![ambiguous]),
        (
            "src/far.rs",
            &["far"],
            vec![bound(
                &far,
                &caller,
                SourceEdgeKind::Calls,
                EdgeProvenance::Import,
                "caller",
            )],
        ),
        ("src/target.rs", &["target"], vec![]),
        ("src/other.rs", &["other"], vec![]),
    ])
}

#[test]
fn impact_reaches_a_node_through_a_candidate_at_the_candidate_trust() {
    let graph = candidate_chain();

    let result = impact_with(
        &graph,
        &func_id("src/target.rs", "target"),
        &ImpactOptions::default(),
    );

    assert_eq!(
        hit_ids(&result),
        vec!["src/caller.rs#function:caller", "src/far.rs#function:far"]
    );
    for hit in &result.hits {
        assert_eq!(hit.min_confidence, AMBIGUOUS_CANDIDATE_CONFIDENCE);
        assert_eq!(hit.weakest_provenance, EdgeProvenance::Syntax);
        assert!(hit.via_candidates, "{} rests on a candidate step", hit.id);
    }
}

#[test]
fn a_bound_path_is_not_flagged_as_via_candidates() {
    let target = func_id("src/target.rs", "target");
    let caller = func_id("src/caller.rs", "caller");
    let graph = graph_from(vec![
        (
            "src/caller.rs",
            &["caller"],
            vec![bound(
                &caller,
                &target,
                SourceEdgeKind::Calls,
                EdgeProvenance::LocalName,
                "target",
            )],
        ),
        ("src/target.rs", &["target"], vec![]),
    ]);

    let result = impact_with(&graph, &target, &ImpactOptions::default());

    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].min_confidence, LOCAL_NAME_CONFIDENCE);
    assert!(!result.hits[0].via_candidates);
}

#[test]
fn follow_candidates_false_excludes_the_candidate_path() {
    let graph = candidate_chain();
    let options = ImpactOptions {
        follow_candidates: false,
        ..ImpactOptions::default()
    };

    let result = impact_with(&graph, &func_id("src/target.rs", "target"), &options);

    assert!(result.hits.is_empty());
}

#[test]
fn a_min_confidence_above_the_candidate_trust_requires_bound_edges() {
    let graph = candidate_chain();
    let target = func_id("src/target.rs", "target");
    let strict = ImpactOptions {
        min_confidence: 0.3,
        ..ImpactOptions::default()
    };
    let at_trust = ImpactOptions {
        min_confidence: AMBIGUOUS_CANDIDATE_CONFIDENCE,
        ..ImpactOptions::default()
    };

    assert!(impact_with(&graph, &target, &strict).hits.is_empty());
    assert_eq!(impact_with(&graph, &target, &at_trust).hits.len(), 2);
}

#[test]
fn a_provenance_filter_excludes_other_classes() {
    let target = func_id("src/target.rs", "target");
    let imported = func_id("src/imported.rs", "imported");
    let unique = func_id("src/unique.rs", "unique");
    let graph = graph_from(vec![
        (
            "src/imported.rs",
            &["imported"],
            vec![bound(
                &imported,
                &target,
                SourceEdgeKind::Calls,
                EdgeProvenance::Import,
                "target",
            )],
        ),
        (
            "src/unique.rs",
            &["unique"],
            vec![bound(
                &unique,
                &target,
                SourceEdgeKind::Calls,
                EdgeProvenance::UniqueName,
                "target",
            )],
        ),
        ("src/target.rs", &["target"], vec![]),
    ]);
    let options = ImpactOptions {
        provenances: vec![EdgeProvenance::Import],
        ..ImpactOptions::default()
    };

    let result = impact_with(&graph, &target, &options);

    assert_eq!(hit_ids(&result), vec![imported.as_str()]);
}

#[test]
fn a_path_prefix_filters_after_traversal_and_counts_what_it_removed() {
    let target = func_id("src/target.rs", "target");
    let bridge = func_id("tests/bridge.rs", "bridge");
    let far = func_id("src/far.rs", "far");
    let graph = graph_from(vec![
        (
            "tests/bridge.rs",
            &["bridge"],
            vec![bound(
                &bridge,
                &target,
                SourceEdgeKind::Calls,
                EdgeProvenance::LocalName,
                "target",
            )],
        ),
        (
            "src/far.rs",
            &["far"],
            vec![bound(
                &far,
                &bridge,
                SourceEdgeKind::Calls,
                EdgeProvenance::LocalName,
                "bridge",
            )],
        ),
        ("src/target.rs", &["target"], vec![]),
    ]);
    let options = ImpactOptions {
        path_prefix: Some("src/".to_string()),
        ..ImpactOptions::default()
    };

    let result = impact_with(&graph, &target, &options);

    assert_eq!(
        hit_ids(&result),
        vec![far.as_str()],
        "the walk crossed the filtered-out bridge"
    );
    assert_eq!(result.filtered_out, 1);
    assert_eq!(result.suppressed, 0);
}
