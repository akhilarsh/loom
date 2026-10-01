//! The per-tick retry of a blocked merge skips an attempt whose inputs did
//! not change, because each attempt writes git objects into the main checkout.

use std::path::Path;

use tempfile::TempDir;

use super::super::resolver_spawn::test_fixtures::{
    git_ok, orchestrator_with_conflict, repo_with_stage_branches,
};
use crate::models::stage::{Stage, StageStatus};
use crate::orchestrator::core::Orchestrator;
use crate::verify::transitions::{load_stage, update_stage};

const ID: &str = "s";

/// A stage `s` with a worktree on `loom/s` holding one commit, an
/// orchestrator over it, and an untracked `stage.txt` in the main checkout
/// that blocks the merge. The state directories are excluded from the
/// checkout's status, as they are in a real project.
fn blocked_stage() -> (TempDir, Orchestrator) {
    let repo = repo_with_stage_branches(&[]);
    let root = repo.path();
    std::fs::write(root.join(".git/info/exclude"), ".loom/\n.worktrees/\n").unwrap();
    let worktree = root.join(".worktrees").join(ID);
    git_ok(
        root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "loom/s",
            worktree.to_str().unwrap(),
        ],
    );
    std::fs::write(worktree.join("stage.txt"), "stage").unwrap();
    git_ok(&worktree, &["add", "stage.txt"]);
    git_ok(&worktree, &["commit", "-q", "-m", "stage"]);

    let mut orchestrator = orchestrator_with_conflict(root, ID);
    update_stage(ID, &orchestrator.config.work_dir, |stage| {
        stage.status = StageStatus::Completed;
        Ok(())
    })
    .unwrap();
    std::fs::write(root.join("stage.txt"), "operator's own file").unwrap();
    assert!(!orchestrator.try_auto_merge(ID));
    (repo, orchestrator)
}

fn on_disk(orchestrator: &Orchestrator) -> Stage {
    load_stage(ID, &orchestrator.config.work_dir).unwrap()
}

fn stage_file_text(orchestrator: &Orchestrator) -> String {
    let stages = orchestrator.config.work_dir.join("stages");
    let file = crate::fs::stage_files::find_stage_file(&stages, ID)
        .unwrap()
        .unwrap();
    std::fs::read_to_string(file).unwrap()
}

/// The `count` and `in-pack` lines of `git count-objects -v`.
fn object_counts(root: &Path) -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["count-objects", "-v"])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.starts_with("count:") || line.starts_with("in-pack:"))
        .map(String::from)
        .collect()
}

#[test]
fn an_unchanged_block_is_not_retried_and_writes_no_object() {
    let (repo, mut orchestrator) = blocked_stage();

    orchestrator.retry_blocked_merge(&on_disk(&orchestrator));
    let objects = object_counts(repo.path());
    let stage_file = stage_file_text(&orchestrator);
    orchestrator.retry_blocked_merge(&on_disk(&orchestrator));

    assert_eq!(object_counts(repo.path()), objects);
    assert_eq!(stage_file_text(&orchestrator), stage_file);
    assert_eq!(on_disk(&orchestrator).status, StageStatus::MergeBlocked);
    assert!(orchestrator.blocked_merge_inputs.contains_key(ID));
}

#[test]
fn a_cleared_block_is_retried_and_lands() {
    let (repo, mut orchestrator) = blocked_stage();
    orchestrator.retry_blocked_merge(&on_disk(&orchestrator));
    orchestrator.retry_blocked_merge(&on_disk(&orchestrator));

    git_ok(repo.path(), &["stash", "push", "-q", "--include-untracked"]);
    orchestrator.retry_blocked_merge(&on_disk(&orchestrator));

    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::Completed);
    assert!(stage.merged);
    assert!(!orchestrator.blocked_merge_inputs.contains_key(ID));
}
