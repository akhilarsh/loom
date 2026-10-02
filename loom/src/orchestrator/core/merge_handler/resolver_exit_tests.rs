//! The resolver's permission approvals reach the loom-owned list before its
//! worktree is removed.

use super::super::resolver_spawn::test_fixtures::{
    orchestrator_with_conflict, repo_with_stage_branches,
};
use crate::fs::permissions::approved::approved_path;

const RULE: &str = "Bash(cargo test:*)";

#[test]
fn approvals_in_the_resolver_worktree_are_recorded() {
    let repo = repo_with_stage_branches(&["s"]);
    let orchestrator = orchestrator_with_conflict(repo.path(), "s");
    let claude = repo.path().join(".worktrees").join("s").join(".claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::write(
        claude.join("settings.local.json"),
        format!(r#"{{"permissions":{{"allow":["{RULE}"]}}}}"#),
    )
    .unwrap();

    orchestrator.fold_back_resolver_permissions("s");

    let approved = std::fs::read_to_string(approved_path(&orchestrator.config.work_dir)).unwrap();
    assert!(approved.contains(RULE), "{approved}");
}

#[test]
fn a_missing_worktree_folds_nothing_back_and_does_not_fail() {
    let repo = repo_with_stage_branches(&["s"]);
    let orchestrator = orchestrator_with_conflict(repo.path(), "s");
    std::fs::remove_dir_all(repo.path().join(".worktrees").join("s")).unwrap();

    orchestrator.fold_back_resolver_permissions("s");

    assert!(!approved_path(&orchestrator.config.work_dir).exists());
}
