//! Trust caveats on packed items.

use super::source_fixtures::{full_node, graph, graph_with_node, node, source_candidate};
use crate::context::graph_store::ResolvedGraph;
use crate::context::pack::{pack, PackRequest};
use crate::context::rank::{EdgeDirection, NeighborVia};
use crate::context::render::literal_text_tokens;
use crate::context::schema::{
    Channel, ContextItem, FileCoverage, Freshness, RequiredRepresentation, SelectionReason,
    SourceNodeKind, TextSearchHint, BRIEF_FRAME_TOKENS,
};
use crate::context::source_graph::{EdgeProvenance, SourceEdgeKind};

const PARTIAL: &str = "src/a.rs#function:alpha";
const FULL: &str = "src/b.rs#function:beta";

fn request() -> PackRequest {
    PackRequest {
        query: "query".into(),
        scope: vec![Channel::Source],
        budget_tokens: BRIEF_FRAME_TOKENS + 400,
        structural_freshness: Freshness::default(),
        semantic_freshness: Freshness {
            revision: "test-rev".into(),
            ..Freshness::default()
        },
        dropped_terms: Vec::new(),
        surviving_terms: vec!["query".to_string()],
        required_representation: RequiredRepresentation::Full,
        degraded: None,
        text_search: None,
    }
}

#[test]
fn items_built_today_carry_no_caveat() {
    let node = full_node(
        "src/a.rs#function:widget",
        "src/a.rs",
        &["widget"],
        "fn widget()",
    );
    let ranked = source_candidate("src/a.rs#function:widget", 2.5, 4);

    let packed = pack(&request(), &[ranked], &[], Some(&graph_with_node(node)));

    assert_eq!(packed.items.len(), 1);
    assert_eq!(packed.items[0].caveat, None);
}

/// One partially covered node and one fully covered node, in separate files.
fn mixed_graph() -> ResolvedGraph {
    let partial = node(
        PARTIAL,
        "src/a.rs",
        &["alpha"],
        "fn alpha()",
        SourceNodeKind::Function,
        FileCoverage::Partial {
            detail: "unnamed matches".to_string(),
        },
    );
    let full = full_node(FULL, "src/b.rs", &["beta"], "fn beta()");
    graph(vec![("src/a.rs", vec![partial]), ("src/b.rs", vec![full])])
}

/// Both nodes at the same score, the partial one listed first.
fn pack_mixed(request: &PackRequest) -> Vec<ContextItem> {
    let ranked = [
        source_candidate(PARTIAL, 2.5, 4),
        source_candidate(FULL, 2.5, 4),
    ];
    pack(request, &ranked, &[], Some(&mixed_graph())).items
}

fn item<'a>(items: &'a [ContextItem], id: &str) -> &'a ContextItem {
    items
        .iter()
        .find(|item| item.id.as_str() == id)
        .unwrap_or_else(|| panic!("{id} must be packed: {items:#?}"))
}

#[test]
fn partial_coverage_carries_a_caveat_and_ranks_below_an_equal_full_node() {
    let items = pack_mixed(&request());

    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].id.as_str(),
        FULL,
        "the full node must lead: {items:#?}"
    );
    assert_eq!(items[0].caveat, None);
    assert_eq!(items[1].caveat.as_deref(), Some("partial coverage"));
    assert!(
        (items[1].score - 1.5).abs() < 1e-6,
        "0.6 of 2.5, got {}",
        items[1].score
    );
}

#[test]
fn a_stale_pack_marks_every_source_item() {
    let mut stale = request();
    stale.semantic_freshness.stale = true;

    let items = pack_mixed(&stale);

    assert_eq!(item(&items, FULL).caveat.as_deref(), Some("snapshot stale"));
    assert_eq!(
        item(&items, PARTIAL).caveat.as_deref(),
        Some("partial coverage; snapshot stale")
    );
}

#[test]
fn a_stale_pack_demotes_source_scores() {
    let mut stale = request();
    stale.semantic_freshness.stale = true;

    let items = pack_mixed(&stale);

    assert!((item(&items, FULL).score - 1.5).abs() < 1e-6);
    assert!((item(&items, PARTIAL).score - 0.9).abs() < 1e-6);
}

#[test]
fn a_never_built_snapshot_names_its_state() {
    let mut never_built = request();
    never_built.semantic_freshness = Freshness::default();

    let items = pack_mixed(&never_built);

    assert_eq!(
        item(&items, FULL).caveat.as_deref(),
        Some("snapshot never built")
    );
}

#[test]
fn a_current_pack_carries_no_snapshot_caveat() {
    let items = pack_mixed(&request());

    assert_eq!(item(&items, FULL).caveat, None);
    assert_eq!(
        item(&items, PARTIAL).caveat.as_deref(),
        Some("partial coverage")
    );
}

#[test]
fn a_caveat_is_priced_into_the_item() {
    let mut stale = request();
    stale.semantic_freshness.stale = true;

    let current = pack_mixed(&request());
    let stale = pack_mixed(&stale);

    assert!(item(&stale, FULL).token_count >= item(&current, FULL).token_count);
}

#[test]
fn a_neighbour_item_carries_its_explanation() {
    let mut ranked = source_candidate(FULL, 0.5, 4);
    ranked.via = Some(NeighborVia {
        seed: "src/a.rs#function:alpha".to_string(),
        edge_kind: SourceEdgeKind::Calls,
        direction: EdgeDirection::Incoming,
        provenance: EdgeProvenance::LocalName,
        site_line: Some(9),
    });

    let packed = pack(&request(), &[ranked], &[], Some(&mixed_graph()));

    assert_eq!(
        packed.items[0].explanation.as_deref(),
        Some("called by `alpha` at src/a.rs:L9")
    );
}

/// Both nodes required by id, so `reserve` (not the optional pass) admits them.
fn required_pair() -> [crate::context::rank::RankedCandidate; 2] {
    [PARTIAL, FULL].map(|id| {
        let mut candidate = source_candidate(id, 2.5, 4);
        candidate.reasons = vec![SelectionReason::ExplicitId];
        candidate
    })
}

#[test]
fn a_required_source_item_is_priced_with_its_snapshot_caveat() {
    let ranked = required_pair();
    let mut stale = request();
    stale.semantic_freshness.stale = true;
    let unlimited = |request: &PackRequest| {
        let request = PackRequest {
            budget_tokens: 1_000_000,
            ..request.clone()
        };
        pack(&request, &ranked, &[], Some(&mixed_graph())).estimated_tokens
    };
    let current_cost = unlimited(&request());
    assert!(
        unlimited(&stale) > current_cost,
        "the caveat must cost tokens for this test to bite"
    );

    // Room for both items without the caveat, not with it.
    stale.budget_tokens = current_cost;
    let packed = pack(&stale, &ranked, &[], Some(&mixed_graph()));

    assert!(
        packed.within_budget(),
        "pack claims {} tokens against a budget of {}: {packed:#?}",
        packed.estimated_tokens,
        packed.budget_tokens
    );
    assert!(
        packed.items.iter().all(|item| item
            .caveat
            .as_deref()
            .is_some_and(|c| c.ends_with("snapshot stale"))),
        "{:#?}",
        packed.items
    );
}

#[test]
fn a_text_search_hint_is_charged_and_held_back_from_the_budget() {
    let ranked = [source_candidate(FULL, 2.5, 4)];
    let hint = TextSearchHint::for_pattern("connection refused");
    let hinted = PackRequest {
        text_search: Some(hint.clone()),
        ..request()
    };
    let without = pack(&request(), &ranked, &[], Some(&mixed_graph()));
    let with = pack(&hinted, &ranked, &[], Some(&mixed_graph()));

    assert_eq!(
        with.estimated_tokens,
        without.estimated_tokens + literal_text_tokens(&hint)
    );

    // Exactly enough for the item and the line, then one token short of it.
    let exact = PackRequest {
        budget_tokens: with.estimated_tokens,
        ..hinted.clone()
    };
    let fits = pack(&exact, &ranked, &[], Some(&mixed_graph()));
    assert_eq!(fits.items.len(), 1);
    assert!(fits.within_budget());

    let short = PackRequest {
        budget_tokens: with.estimated_tokens - 1,
        ..hinted
    };
    let dropped = pack(&short, &ranked, &[], Some(&mixed_graph()));
    assert!(dropped.items.is_empty(), "{:#?}", dropped.items);
    assert!(dropped.within_budget());
}
