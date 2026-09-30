//! A layer written under another schema, or one that no longer parses, is
//! rebuilt by `ensure_snapshot` instead of being served or wedging the cache.

use super::snapshot::{ensure_snapshot, SnapshotAction, SnapshotPolicy};
use super::tests_snapshot::{
    init_repo, lock_base_dir_read_only, restore_after_persist_probe, stores,
};
use crate::context::graph_store::GraphLayer;
use crate::context::local_overlay::local_overlay_key;
use serial_test::serial;
use std::path::Path;

/// Rewrite the layer at `path` under schema 0, as a pre-contract build left it.
fn downgrade_schema(path: &Path) {
    let mut layer: GraphLayer = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    layer.schema_version = 0;
    std::fs::write(path, serde_json::to_vec(&layer).unwrap()).unwrap();
}

#[test]
#[serial]
fn stale_schema_base_is_rebuilt_not_reused() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    let first = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    assert_eq!(first.action, SnapshotAction::Rebuilt, "{first:#?}");
    let base = graph_store.base_path(&first.revision);
    downgrade_schema(&base);

    let second = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);

    assert_eq!(second.action, SnapshotAction::Rebuilt, "{second:#?}");
    assert_eq!(second.counters.files_reused, 0, "a stale layer was reused");
    let layer = graph_store.load_base(&first.revision).unwrap().unwrap();
    assert!(layer.has_current_schema());
}

#[test]
#[serial]
fn unparseable_base_is_rebuilt_and_rewritten() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    let first = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    let base = graph_store.base_path(&first.revision);
    std::fs::write(&base, b"{\"revision\": ").unwrap();

    let second = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);

    assert_eq!(second.action, SnapshotAction::Rebuilt, "{second:#?}");
    let layer = graph_store.load_base(&first.revision).unwrap().unwrap();
    assert!(layer.has_current_schema());
    assert!(layer.files.contains_key("src.rs"));
}

#[test]
#[serial]
fn stale_schema_overlay_is_rebuilt() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    std::fs::write(root.join("src.rs"), "fn committed() {}\nfn dirty() {}\n").unwrap();
    let first = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::LocalCurrent);
    assert_ne!(first.action, SnapshotAction::Unavailable, "{first:#?}");
    let (plan, stage) = local_overlay_key(root);
    let overlay = graph_store.overlay_path(&plan, &stage);
    downgrade_schema(&overlay);

    let second = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::LocalCurrent);

    assert_ne!(second.action, SnapshotAction::Reused, "{second:#?}");
    assert_ne!(second.action, SnapshotAction::Unavailable, "{second:#?}");
    let layer = graph_store.load_overlay(&plan, &stage).unwrap().unwrap();
    assert!(layer.has_current_schema());
    assert!(layer.files.contains_key("src.rs"));
}

#[test]
#[serial]
fn stale_base_in_a_read_only_cache_is_rebuilt_and_served_from_memory() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    let first = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    let base = graph_store.base_path(&first.revision);
    downgrade_schema(&base);
    let (base_dir, still_writable) = lock_base_dir_read_only(&graph_store);

    let second = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);

    if restore_after_persist_probe(
        &base_dir,
        still_writable,
        "stale_base_in_a_read_only_cache_is_rebuilt_and_served_from_memory",
    ) {
        return;
    }
    assert_eq!(second.action, SnapshotAction::Rebuilt, "{second:#?}");
    let on_disk: GraphLayer = serde_json::from_slice(&std::fs::read(&base).unwrap()).unwrap();
    assert_eq!(
        on_disk.schema_version, 0,
        "the read-only base file was written"
    );
    let served = graph_store.load_base(&first.revision).unwrap().unwrap();
    assert!(served.has_current_schema(), "the stale base was served");
    assert!(served.files.contains_key("src.rs"));

    let third = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    assert_eq!(third.action, SnapshotAction::Reused, "{third:#?}");
}
