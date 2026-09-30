//! The four graph states a `SnapshotOutcome` reports, the stale base a failed
//! build serves, and the read-only-cache marker.

use super::tests_snapshot::{
    git_ok, init_repo, lock_base_dir_read_only, restore_after_persist_probe, stores,
};
use super::*;
use crate::context::freshness::GraphState;
use crate::context::graph_store::GraphStore;
use serial_test::serial;
use tempfile::TempDir;

fn stage_policy() -> SnapshotPolicy {
    SnapshotPolicy::StageOverlay {
        plan: "plan".to_string(),
        stage: "stage".to_string(),
    }
}

/// Make the stage overlay unreadable: a directory where the layer file
/// belongs fails the read with a non-NotFound error, so the build fails
/// before it touches any base.
fn break_stage_overlay(graph_store: &GraphStore) {
    std::fs::create_dir_all(graph_store.overlay_path("plan", "stage")).unwrap();
}

fn commit_second_revision(root: &Path) {
    std::fs::write(root.join("src.rs"), "fn second() {}\n").unwrap();
    git_ok(root, &["add", "src.rs"]);
    git_ok(root, &["commit", "-m", "second"]);
}

#[test]
#[serial]
fn failed_build_serves_newest_older_base_as_stale() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    let first = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);
    assert_eq!(first.state(), GraphState::Current, "{first:?}");
    assert!(first.persisted);
    commit_second_revision(root);
    break_stage_overlay(&graph_store);

    let outcome = ensure_snapshot(&store, &graph_store, root, stage_policy());

    let head = working_tree(root).unwrap().head;
    assert_ne!(head, first.revision, "the failed build's HEAD is newer");
    assert_eq!(outcome.action, SnapshotAction::Unavailable, "{outcome:?}");
    assert_eq!(outcome.revision, first.revision);
    assert_eq!(outcome.serving, Some(first.revision.clone()));
    assert_eq!(outcome.state(), GraphState::Stale);
    let suffix = format!("; serving stale base {}", short_revision(&first.revision));
    assert!(
        outcome.describe().ends_with(&suffix),
        "{}",
        outcome.describe()
    );
}

#[test]
#[serial]
fn failed_build_without_a_base_is_never_built() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    break_stage_overlay(&graph_store);

    let outcome = ensure_snapshot(&store, &graph_store, root, stage_policy());

    assert_eq!(outcome.action, SnapshotAction::Unavailable, "{outcome:?}");
    assert_eq!(outcome.serving, None);
    assert_eq!(outcome.state(), GraphState::NeverBuilt);
}

#[test]
#[serial]
fn a_directory_without_git_reports_the_unavailable_state() {
    let temp = TempDir::new().unwrap();
    let (store, graph_store) = stores(&temp);

    let outcome = ensure_snapshot(
        &store,
        &graph_store,
        temp.path(),
        SnapshotPolicy::LocalCurrent,
    );

    assert_eq!(outcome.state(), GraphState::Unavailable);
    assert_eq!(outcome.serving, None);
}

#[test]
#[serial]
fn read_only_cache_reports_not_persisted() {
    let temp = init_repo();
    let root = temp.path();
    let (store, graph_store) = stores(&temp);
    let (base_dir, still_writable) = lock_base_dir_read_only(&graph_store);

    let outcome = ensure_snapshot(&store, &graph_store, root, SnapshotPolicy::BaseOnly);

    let test = "read_only_cache_reports_not_persisted";
    if restore_after_persist_probe(&base_dir, still_writable, test) {
        return;
    }
    assert_eq!(outcome.state(), GraphState::Current, "{outcome:?}");
    assert!(!outcome.persisted);
    let line = outcome.describe();
    assert!(
        line.ends_with("; not persisted (cache read-only)"),
        "{line}"
    );
}
