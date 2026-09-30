//! The eval refreshes the local source-graph snapshot once before any case
//! runs, and keeps going when that fails.

use super::{ensure_local_snapshot, refresh_source_graph};
use crate::context::freshness::GraphState;
use crate::context::refresh::SnapshotAction;
use serial_test::serial;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Run one git command with ambient global/system config neutralized, so a
/// developer's or CI runner's `~/.gitconfig` cannot change test behavior.
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", root.join(".loom-test-no-global"))
        .env("GIT_CONFIG_SYSTEM", root.join(".loom-test-no-system"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A git repo, with one committed Rust file when `committed` and no HEAD at
/// all otherwise.
fn repo(committed: bool) -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "t@t.com"]);
    git(root, &["config", "user.name", "t"]);
    if committed {
        std::fs::write(root.join("src.rs"), "pub fn refreshed() {}\n").unwrap();
        git(root, &["add", "src.rs"]);
        git(root, &["commit", "-m", "seed"]);
    }
    temp
}

#[test]
#[serial]
fn the_local_snapshot_is_built_for_the_committed_tree() {
    let temp = repo(true);

    let outcome = ensure_local_snapshot(temp.path()).unwrap();

    assert_ne!(
        outcome.action,
        SnapshotAction::Unavailable,
        "{}",
        outcome.reason
    );
    assert_eq!(outcome.state(), GraphState::Current);
    assert!(!outcome.revision.is_empty());
}

#[test]
#[serial]
fn a_snapshot_that_cannot_be_built_is_reported_and_the_eval_goes_on() {
    let temp = repo(false);

    let outcome = ensure_local_snapshot(temp.path()).unwrap();
    assert_eq!(outcome.action, SnapshotAction::Unavailable);

    // Returns rather than raising: the cases still run against what exists.
    refresh_source_graph(temp.path());
}
