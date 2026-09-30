//! Tests for cross-file symbol and import resolution.

use super::fixtures::*;
use super::*;
use crate::context::source_graph::{
    SourceNodeKind, LOCAL_NAME_CONFIDENCE, UNIQUE_NAME_CONFIDENCE, UNRESOLVED_TARGET,
};

#[test]
fn a_unique_cross_file_match_retargets_and_raises_confidence() {
    let caller = func_id("src/caller.rs", "invoke");
    let mut graph = graph_from(vec![
        (
            "src/caller.rs",
            &["invoke"],
            seeking(&caller, SourceEdgeKind::Calls, "target"),
        ),
        ("src/defs.rs", &["target"], vec![]),
    ]);

    let stats = resolve_graph(&mut graph);

    let edge = &graph.files["src/caller.rs"].edges[0];
    assert_eq!(edge.to, func_id("src/defs.rs", "target"));
    assert_eq!(edge.confidence, UNIQUE_NAME_CONFIDENCE);
    assert_eq!(
        edge.provenance,
        EdgeProvenance::UniqueName,
        "a name match must never present itself as a parse"
    );
    assert_eq!(stats, expected_stats(1, 0, 0));
}

#[test]
fn two_definitions_of_one_name_stay_unresolved() {
    let caller = func_id("src/caller.rs", "invoke");
    let mut graph = graph_from(vec![
        (
            "src/caller.rs",
            &["invoke"],
            seeking(&caller, SourceEdgeKind::Calls, "target"),
        ),
        ("src/one.rs", &["target"], vec![]),
        ("src/two.rs", &["target"], vec![]),
    ]);
    let before = graph.files["src/caller.rs"].edges[0].clone();

    let stats = resolve_graph(&mut graph);

    let edge = &graph.files["src/caller.rs"].edges[0];
    assert_eq!(edge.to, UNRESOLVED_TARGET);
    assert_eq!(edge.confidence, before.confidence, "ambiguity buys nothing");
    assert_eq!(stats, expected_stats(0, 1, 1));
}

#[test]
fn a_file_stem_colliding_with_a_symbol_is_ambiguity() {
    let mut graph = graph_from(vec![
        (
            "src/app.rs",
            &[],
            seeking("src/app.rs", SourceEdgeKind::References, "language"),
        ),
        ("src/language.rs", &[], vec![]),
        ("src/other.rs", &["language"], vec![]),
    ]);

    let stats = resolve_graph(&mut graph);

    assert_eq!(graph.files["src/app.rs"].edges[0].to, UNRESOLVED_TARGET);
    assert_eq!(stats.ambiguous, 1);
}

#[test]
fn a_local_name_edge_is_untouched_even_when_a_unique_match_exists() {
    let local_edge = edge_at(
        func_id("src/caller.rs", "invoke").as_str(),
        UNRESOLVED_TARGET,
        SourceEdgeKind::Calls,
        EdgeProvenance::LocalName,
        LOCAL_NAME_CONFIDENCE,
    );
    let mut graph = graph_from(vec![
        ("src/caller.rs", &["invoke"], vec![local_edge.clone()]),
        ("src/defs.rs", &["target"], vec![]),
    ]);

    let stats = resolve_graph(&mut graph);

    assert_eq!(graph.files["src/caller.rs"].edges[0], local_edge);
    assert_eq!(stats, expected_stats(0, 0, 1));
    assert_eq!(
        stats.by_provenance,
        BTreeMap::from([("local-name".to_string(), 1)])
    );
}

#[test]
fn a_containment_edge_is_counted_but_never_guessed_at() {
    let mut graph = graph_from(vec![
        (
            "src/app.rs",
            &[],
            seeking("src/app.rs", SourceEdgeKind::Contains, "orphan"),
        ),
        ("src/defs.rs", &["orphan"], vec![]),
    ]);

    let stats = resolve_graph(&mut graph);

    assert_eq!(graph.files["src/app.rs"].edges[0].to, UNRESOLVED_TARGET);
    assert_eq!(stats, expected_stats(0, 0, 1));
}

#[test]
fn an_impl_block_is_not_a_rival_definition_of_its_type() {
    let mut graph = graph_of(vec![
        (
            "src/app.rs",
            source_file(
                "src/app.rs",
                &[],
                seeking("src/app.rs", SourceEdgeKind::References, "Widget"),
            ),
        ),
        (
            "src/defs.rs",
            mixed_file(
                "src/defs.rs",
                &[
                    (SourceNodeKind::Type, "Widget"),
                    (SourceNodeKind::Implementation, "Widget"),
                ],
                vec![],
            ),
        ),
        (
            "src/more.rs",
            mixed_file(
                "src/more.rs",
                &[(SourceNodeKind::Implementation, "Widget")],
                vec![],
            ),
        ),
    ]);

    let stats = resolve_graph(&mut graph);

    assert_eq!(
        graph.files["src/app.rs"].edges[0].to,
        scoped_id("src/defs.rs", SourceNodeKind::Type, "Widget"),
        "an impl block attaches to the type; it does not compete with it"
    );
    assert_eq!(stats, expected_stats(1, 0, 0));
}

#[test]
fn a_name_carried_only_by_impl_blocks_resolves_to_nothing() {
    let impls =
        |path: &str| mixed_file(path, &[(SourceNodeKind::Implementation, "Orphan")], vec![]);
    let referrer = || {
        source_file(
            "src/app.rs",
            &[],
            seeking("src/app.rs", SourceEdgeKind::References, "Orphan"),
        )
    };

    let mut contested = graph_of(vec![
        ("src/app.rs", referrer()),
        ("src/one.rs", impls("src/one.rs")),
        ("src/two.rs", impls("src/two.rs")),
    ]);
    let stats = resolve_graph(&mut contested);
    assert_eq!(contested.files["src/app.rs"].edges[0].to, UNRESOLVED_TARGET);
    assert_eq!(
        stats,
        expected_stats(0, 0, 1),
        "impl blocks define nothing, so two of them are no candidate set either"
    );

    let mut lone = graph_of(vec![
        ("src/app.rs", referrer()),
        ("src/one.rs", impls("src/one.rs")),
    ]);
    let stats = resolve_graph(&mut lone);
    assert_eq!(lone.files["src/app.rs"].edges[0].to, UNRESOLVED_TARGET);
    assert_eq!(
        stats,
        expected_stats(0, 0, 1),
        "one impl block is not a contest, just nothing to resolve to"
    );
}

#[test]
fn resolution_is_deterministic_and_idempotent() {
    let caller = func_id("src/caller.rs", "invoke");
    let build = || {
        graph_from(vec![
            (
                "src/caller.rs",
                &["invoke"],
                vec![
                    unresolved_edge(caller.as_str(), SourceEdgeKind::Calls, "target"),
                    unresolved_edge(caller.as_str(), SourceEdgeKind::Calls, "nowhere"),
                ],
            ),
            ("src/defs.rs", &["target"], vec![]),
        ])
    };

    let (mut first, mut second) = (build(), build());
    let stats = resolve_graph(&mut first);
    assert_eq!(stats, resolve_graph(&mut second));
    assert_eq!(first, second);

    let again = resolve_graph(&mut first);
    assert_eq!(
        again,
        expected_stats(1, 0, 1),
        "an already-resolved edge is not re-resolved, and the residue is stable"
    );
}

#[test]
fn the_symbol_index_reports_files_under_both_name_and_stem() {
    let graph = graph_from(vec![("src/language.rs", &["detect"], vec![])]);
    let index = SymbolIndex::build(&graph);
    let keys = &mut BTreeSet::new();

    assert_eq!(
        index.lookup("rust", "language.rs", keys),
        ["src/language.rs".to_string()]
    );
    assert_eq!(
        index.lookup("rust", "language", keys),
        ["src/language.rs".to_string()]
    );
    assert_eq!(
        index.lookup("rust", "detect", keys),
        [func_id("src/language.rs", "detect")]
    );
    assert!(index.lookup("rust", "absent", keys).is_empty());
}
