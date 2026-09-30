//! The resolved view across `ensure_snapshot`: written once per snapshot,
//! read back by identity, and removed with the layer it describes.

use super::tests_snapshot::{init_repo, stores};
use super::*;
use crate::context::graph_store::GraphStore;
use crate::context::view::{ResolvedView, ViewIdentity, ViewOrigin};
use serial_test::serial;

/// A store over the same directories, standing in for a second process: its
/// in-process view cache is empty.
fn second_process(temp: &tempfile::TempDir) -> GraphStore {
    stores(temp).1
}

#[test]
#[serial]
fn ensure_snapshot_writes_the_base_view_and_a_second_load_reads_it() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);

    let first = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);

    assert!(first.counters.bytes_read > 0, "{:?}", first.counters);
    let identity = ViewIdentity::current(&first.revision, "");
    assert!(graph_store.view_path(&identity, None).is_file());
    // The process that materialized it hands it over without parsing.
    let held = graph_store.view(&first.revision, None).unwrap();
    assert_eq!(held.origin, ViewOrigin::Built);

    let second = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    assert_eq!(second.action, SnapshotAction::Reused);
    let loaded = second_process(&temp).view(&second.revision, None).unwrap();
    assert_eq!(loaded.origin, ViewOrigin::Materialized);
}

#[test]
#[serial]
fn a_reused_snapshot_leaves_the_view_file_for_the_reader() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    let fresh = second_process(&temp);

    let reused = ensure_snapshot(&store, &fresh, root, SnapshotPolicy::BaseOnly);

    assert_eq!(reused.action, SnapshotAction::Reused);
    let view = fresh.view(&reused.revision, None).unwrap();
    assert_eq!(view.origin, ViewOrigin::Materialized);
}

#[test]
#[serial]
fn pruning_a_base_removes_its_views() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    let outcome = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    let path = graph_store.view_path(&ViewIdentity::current(&outcome.revision, ""), None);
    assert!(path.is_file());

    graph_store.prune_base_graphs(0, &[]).unwrap();

    assert!(!graph_store.base_path(&outcome.revision).exists());
    assert!(!path.exists(), "the view outlived its base");
}

#[test]
#[serial]
fn discarding_an_overlay_removes_its_view() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    std::fs::write(root.join("src.rs"), "fn edited() {}\n").unwrap();

    let outcome = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::LocalCurrent);

    let (plan, stage) = outcome
        .overlay
        .clone()
        .expect("a dirty tree has an overlay");
    let overlay = Some((plan.as_str(), stage.as_str()));
    let identity = ViewIdentity::current(&outcome.revision, &outcome.generation);
    let view_path = graph_store.view_path(&identity, overlay);
    assert!(
        view_path.is_file(),
        "no overlay view at {}",
        view_path.display()
    );
    assert!(graph_store
        .view_path(&ViewIdentity::current(&outcome.revision, ""), None)
        .is_file());

    graph_store.discard_overlay(&plan, &stage).unwrap();

    assert!(!view_path.exists());
    assert!(!graph_store.overlay_path(&plan, &stage).exists());
}

/// Set the modification time of `path` a minute back, so it predates what
/// was written after it even on a coarse filesystem clock.
fn backdate(path: &std::path::Path) {
    let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    std::fs::File::open(path)
        .unwrap()
        .set_modified(earlier)
        .unwrap();
}

#[test]
#[serial]
fn an_overlay_layer_rewritten_without_its_view_is_rematerialized() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    std::fs::write(root.join("src.rs"), "fn edited() {}\n").unwrap();
    let first = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::LocalCurrent);
    let (plan, stage) = first.overlay.expect("a dirty tree has an overlay");
    let identity = ViewIdentity::current(&first.revision, &first.generation);
    let view_path = graph_store.view_path(&identity, Some((plan.as_str(), stage.as_str())));
    // A reconcile that does not materialize, as a merge runs, rewrites the
    // layer for the next generation after the view was written.
    std::fs::write(root.join("src.rs"), "fn edited_again() {}\n").unwrap();
    let scope = SourceGraphScope::Overlay { plan, stage };
    reconcile_source_graph(&store, &graph_store, root, scope).unwrap();
    backdate(&view_path);

    let second = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::LocalCurrent);

    assert_eq!(second.action, SnapshotAction::Reused, "{second:#?}");
    assert_ne!(second.generation, first.generation);
    let persisted: ResolvedView = serde_json::from_slice(&std::fs::read(&view_path).unwrap())
        .expect("the overlay view parses");
    assert_eq!(persisted.identity.overlay_generation, second.generation);
}

#[test]
#[serial]
fn an_edited_tree_rewrites_the_overlay_view_for_its_new_generation() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    std::fs::write(root.join("src.rs"), "fn edited() {}\n").unwrap();
    ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::LocalCurrent);
    std::fs::write(root.join("src.rs"), "fn edited_again() {}\n").unwrap();

    let outcome = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::LocalCurrent);

    let (plan, stage) = outcome
        .overlay
        .clone()
        .expect("a dirty tree has an overlay");
    let view = graph_store
        .view(&outcome.revision, Some((plan.as_str(), stage.as_str())))
        .unwrap();
    assert_eq!(view.identity.overlay_generation, outcome.generation);
    let names: Vec<&str> = view
        .graph
        .nodes()
        .map(|node| node.scope.last().map_or("", String::as_str))
        .collect();
    assert!(names.contains(&"edited_again"), "{names:?}");
}
