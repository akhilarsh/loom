//! The delivery surface a [`StageQuery`] is built for, and the source windows
//! a pack carries on the surfaces that allow them.

use crate::context::extract::{extract_file, registry};
use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::render::rendered_item_tokens;
use crate::context::retrieve::windows::attach_windows;
use crate::context::retrieve::{StageQuery, Surface};
use crate::context::schema::{
    Channel, ChunkId, Confidence, ContextItem, ContextPack, Freshness, ItemKind, LifecycleState,
    OmissionSummary, SelectionReason, SourcePointer,
};
use crate::context::source_graph::FileCoverage;
use std::collections::BTreeSet;
use std::path::Path;
use tempfile::TempDir;

const THREE_FUNCTIONS: &str = "pub fn alpha() {}\npub fn beta() {}\npub fn gamma() {}\n";

#[test]
fn stage_query_defaults_to_the_stage_brief_surface() {
    let query = StageQuery::new(".", "a question");

    assert_eq!(query.surface, Surface::StageBrief);
    assert_eq!(query.with_surface(Surface::Hook).surface, Surface::Hook);
}

/// A directory holding `src/lib.rs` and the graph a build of it would produce.
fn fixture() -> (TempDir, ResolvedGraph) {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("src")).expect("create dirs");
    std::fs::write(temp.path().join("src/lib.rs"), THREE_FUNCTIONS).expect("write fixture");
    let extraction = extract_file(
        &registry(),
        Path::new("src/lib.rs"),
        THREE_FUNCTIONS.as_bytes(),
    );
    let mut graph = ResolvedGraph {
        base_revision: String::new(),
        overlaid: BTreeSet::new(),
        files: Default::default(),
    };
    graph.files.insert(
        "src/lib.rs".to_string(),
        FileEntry::from_extraction(THREE_FUNCTIONS.as_bytes(), extraction),
    );
    (temp, graph)
}

fn source_item(name: &str, reason: SelectionReason) -> ContextItem {
    let mut item = ContextItem {
        id: ChunkId::from(format!("src/lib.rs#function:{name}")),
        kind: ItemKind::SourceNode,
        pointer: SourcePointer {
            path: "src/lib.rs".into(),
            anchor: String::new(),
            line_start: Some(1),
            line_end: Some(1),
        },
        summary: format!("function {name}"),
        source: Channel::Source,
        token_count: 0,
        score: 1.0,
        reasons: vec![reason],
        confidence: Confidence::High,
        state: LifecycleState::Active,
        content_hash: format!("sha256:{name}"),
        excerpt: None,
        truncated: false,
        matched_term_count: 1,
        explanation: None,
        caveat: None,
        window: None,
    };
    item.token_count = rendered_item_tokens(&item);
    item
}

fn pack_of(items: Vec<ContextItem>, semantic: Freshness) -> ContextPack {
    let mut pack = ContextPack {
        query: "alpha".to_string(),
        scope: Channel::all().to_vec(),
        budget_tokens: 10_000,
        estimated_tokens: 0,
        structural_freshness: Freshness::default(),
        semantic_freshness: semantic,
        items,
        unmet_required: Vec::new(),
        omitted: OmissionSummary::default(),
        dropped_terms: Vec::new(),
        degraded: None,
        text_search: None,
    };
    pack.recompute_estimate();
    pack
}

fn current() -> Freshness {
    Freshness {
        revision: "test-rev".to_string(),
        ..Freshness::default()
    }
}

fn exact_items() -> Vec<ContextItem> {
    ["alpha", "beta", "gamma"]
        .map(|name| source_item(name, SelectionReason::ExactSymbol))
        .to_vec()
}

fn window_count(pack: &ContextPack) -> usize {
    pack.items
        .iter()
        .filter(|item| item.window.is_some())
        .count()
}

#[test]
fn a_current_pack_gets_at_most_two_windows_on_its_first_two_matches() {
    let (temp, graph) = fixture();
    let mut pack = pack_of(exact_items(), current());
    let before = pack.estimated_tokens;

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    assert_eq!(window_count(&pack), 2);
    assert_eq!(pack.items[0].window.as_deref(), Some("pub fn alpha() {}\n"));
    assert_eq!(pack.items[1].window.as_deref(), Some("pub fn beta() {}\n"));
    assert!(pack.items[2].window.is_none());
    assert!(pack.estimated_tokens > before, "the windows are charged");
    assert_eq!(
        pack.items[0].token_count,
        rendered_item_tokens(&pack.items[0])
    );
}

#[test]
fn the_stage_brief_surface_gets_windows_too() {
    let (temp, graph) = fixture();
    let mut pack = pack_of(exact_items(), current());

    attach_windows(&mut pack, &graph, temp.path(), Surface::StageBrief);

    assert_eq!(window_count(&pack), 2);
}

#[test]
fn the_hook_surface_gets_no_window() {
    let (temp, graph) = fixture();
    let mut pack = pack_of(exact_items(), current());

    attach_windows(&mut pack, &graph, temp.path(), Surface::Hook);

    assert_eq!(window_count(&pack), 0);
}

#[test]
fn a_partially_extracted_file_gets_no_window() {
    let (temp, mut graph) = fixture();
    graph.files.get_mut("src/lib.rs").expect("file").coverage = FileCoverage::Partial {
        detail: "one match had no name".to_string(),
    };
    let mut pack = pack_of(exact_items(), current());

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    assert_eq!(window_count(&pack), 0);
}

#[test]
fn a_stale_pack_gets_no_window() {
    let (temp, graph) = fixture();
    let stale = Freshness {
        stale: true,
        ..current()
    };
    let mut pack = pack_of(exact_items(), stale);

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    assert_eq!(window_count(&pack), 0);
}

#[test]
fn an_item_matched_only_lexically_gets_no_window() {
    let (temp, graph) = fixture();
    let mut pack = pack_of(
        vec![source_item("alpha", SelectionReason::Lexical)],
        current(),
    );

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    assert_eq!(window_count(&pack), 0);
}

#[test]
fn a_window_that_would_exceed_the_budget_is_dropped_and_the_pack_is_unchanged() {
    let (temp, graph) = fixture();
    let mut pack = pack_of(exact_items(), current());
    pack.budget_tokens = pack.estimated_tokens;
    let untouched = pack.clone();

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    assert_eq!(pack, untouched);
    assert!(pack.within_budget());
}

#[test]
fn a_changed_file_gets_no_window() {
    let (temp, graph) = fixture();
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn edited() {}\n").expect("edit");
    let mut pack = pack_of(exact_items(), current());

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    assert_eq!(window_count(&pack), 0);
}

#[test]
fn a_window_is_charged_to_the_coverage_included_tokens() {
    let (temp, graph) = fixture();
    let mut pack = pack_of(exact_items(), current());
    pack.omitted.coverage.included_tokens = pack.items.iter().map(|item| item.token_count).sum();
    let before = pack.omitted.coverage.included_tokens;

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    let charged: usize = pack.items.iter().map(|item| item.token_count).sum();
    assert!(charged > before, "the windows cost tokens");
    assert_eq!(pack.omitted.coverage.included_tokens, charged);
}

#[test]
fn a_dropped_window_leaves_the_coverage_included_tokens_alone() {
    let (temp, graph) = fixture();
    let mut pack = pack_of(exact_items(), current());
    pack.omitted.coverage.included_tokens = 77;
    pack.budget_tokens = pack.estimated_tokens;

    attach_windows(&mut pack, &graph, temp.path(), Surface::Cli);

    assert_eq!(pack.omitted.coverage.included_tokens, 77);
}
