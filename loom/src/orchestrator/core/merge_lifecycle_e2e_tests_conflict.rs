//! The conflict half of the merge lifecycle, end to end with real git: a
//! conflict, the resolver's work in the stage worktree, `--resolved` through
//! the relay inbox, the resolver's exit, and the ways a resolution is refused.
//! After every step the operator's checkout is checked against a snapshot.

use super::merge_lifecycle_e2e_tests_support::*;
use crate::fs::inbox::LedgerOutcome;
use crate::models::stage::StageStatus;
use crate::orchestrator::core::merge_resolver_attempts;
use crate::orchestrator::merge_lifecycle::test_support::finish_session;

fn branch_exists(root: &std::path::Path) -> bool {
    git_output(
        root,
        &["rev-parse", "--verify", "--quiet", "refs/heads/loom/s"],
    )
    .status
    .success()
}

/// The operator's own work in the checkout: one staged new file and one
/// unstaged edit to a tracked file.
fn operator_work(root: &std::path::Path) {
    std::fs::write(root.join("staged.txt"), "staged\n").unwrap();
    git(root, &["add", "staged.txt"]);
    std::fs::write(root.join("other.txt"), "other\nunstaged edit\n").unwrap();
}

/// The operator's work is still staged and unstaged as it was, beside the
/// resolution in the working copy.
fn assert_operator_work_survives(root: &std::path::Path) {
    assert_eq!(text_of(root, "seed.txt"), "resolved");
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only"]),
        "staged.txt"
    );
    assert_eq!(text_of(root, "staged.txt"), "staged\n");
    assert_eq!(text_of(root, "other.txt"), "other\nunstaged edit\n");
}

#[test]
fn a_conflict_is_resolved_landed_and_cleaned_up_around_the_operators_work() {
    let repo = repo_with_worktree();
    let root = repo.path();
    make_conflict(root);
    operator_work(root);
    let before = checkout(root);
    let main_before = main_tip(root);
    let stage_head = branch_tip(root);
    let mut orchestrator = orchestrator_with_completed_stage(root);
    let work_dir = orchestrator.config.work_dir.clone();

    assert!(!orchestrator.try_auto_merge(ID));

    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::MergeConflict);
    assert_eq!(stage.completed_commit, Some(stage_head));
    assert_eq!(merge_resolver_attempts(&work_dir, ID), 0);
    assert!(orchestrator.active_sessions.is_empty());
    assert_eq!(main_tip(root), main_before);
    assert_untouched(root, &before);

    resolve_by_merging(root, "resolved");
    assert_untouched(root, &before);

    let (outcome, mut session) = resolve_from_inbox(&mut orchestrator);

    assert_eq!(outcome, LedgerOutcome::Applied);
    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::Completed);
    assert!(stage.merged);
    assert!(worktree(root).is_dir(), "the resolver still runs there");
    assert_eq!(git(root, &["show", "main:seed.txt"]), "resolved");
    assert_advanced_only(root, &before, &["seed.txt"]);
    let landed = checkout(root);
    assert_operator_work_survives(root);

    finish_session(&work_dir, &mut session);
    orchestrator
        .handle_merge_session_completed(&session.id, ID)
        .unwrap();

    assert!(!worktree(root).exists());
    assert!(!branch_exists(root));
    assert_untouched(root, &landed);
    assert_eq!(git(root, &["show", "main:seed.txt"]), "resolved");
}

#[test]
fn a_control_path_branch_is_held_for_review_and_main_does_not_move() {
    let repo = repo_with_worktree();
    let root = repo.path();
    commit_file(&worktree(root), ".claude/é.md", "stage work");
    let main_before = main_tip(root);
    let before = checkout(root);
    let mut orchestrator = orchestrator_with_completed_stage(root);

    assert!(!orchestrator.try_auto_merge(ID));

    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::NeedsHumanReview);
    assert!(!stage.merged);
    assert!(stage.review_reason.unwrap().contains(".claude/é.md"));
    assert_eq!(main_tip(root), main_before);
    assert_untouched(root, &before);
}

#[test]
fn a_resolution_that_adds_a_control_path_is_refused_and_routed_to_review() {
    let repo = repo_with_worktree();
    let root = repo.path();
    make_conflict(root);
    let mut orchestrator = orchestrator_with_completed_stage(root);
    assert!(!orchestrator.try_auto_merge(ID));
    let main_before = main_tip(root);
    let before = checkout(root);
    let wt = worktree(root);
    let merge = git_output(&wt, &["merge", "main"]);
    assert!(!merge.status.success());
    std::fs::write(wt.join("seed.txt"), "resolved").unwrap();
    std::fs::create_dir_all(wt.join(".claude")).unwrap();
    std::fs::write(wt.join(".claude/settings.json"), "{}\n").unwrap();
    git(&wt, &["add", "seed.txt", ".claude/settings.json"]);
    git(&wt, &["commit", "-q", "-m", "resolve"]);

    let (outcome, _session) = resolve_from_inbox(&mut orchestrator);

    assert_eq!(outcome, LedgerOutcome::Refused);
    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::NeedsHumanReview);
    assert!(!stage.merged);
    assert!(stage
        .review_reason
        .unwrap()
        .contains(".claude/settings.json"));
    assert_eq!(main_tip(root), main_before);
    assert_untouched(root, &before);
}

#[test]
fn a_resolver_that_rebases_instead_of_merging_lands_nothing() {
    let repo = repo_with_worktree();
    let root = repo.path();
    make_conflict(root);
    let mut orchestrator = orchestrator_with_completed_stage(root);
    assert!(!orchestrator.try_auto_merge(ID));
    let main_before = main_tip(root);
    let before = checkout(root);
    let wt = worktree(root);
    let rebase = git_output(&wt, &["rebase", "main"]);
    assert!(!rebase.status.success(), "the rebase must conflict");
    std::fs::write(wt.join("seed.txt"), "resolved").unwrap();
    git(&wt, &["add", "seed.txt"]);
    git(&wt, &["-c", "core.editor=true", "rebase", "--continue"]);

    let (outcome, mut session) = resolve_from_inbox(&mut orchestrator);

    assert_eq!(outcome, LedgerOutcome::Refused);
    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::MergeConflict);
    assert!(!stage.merged);
    assert_eq!(main_tip(root), main_before);
    assert_untouched(root, &before);

    let work_dir = orchestrator.config.work_dir.clone();
    finish_session(&work_dir, &mut session);
    orchestrator
        .handle_merge_session_completed(&session.id, ID)
        .unwrap();

    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::MergeConflict);
    assert!(!stage.merged);
    assert!(worktree(root).is_dir());
    assert_eq!(main_tip(root), main_before);
    assert_untouched(root, &before);
}

#[test]
fn a_target_that_moved_during_resolution_still_lands_with_both_changes() {
    let repo = repo_with_worktree();
    let root = repo.path();
    make_conflict(root);
    let mut orchestrator = orchestrator_with_completed_stage(root);
    assert!(!orchestrator.try_auto_merge(ID));
    resolve_by_merging(root, "resolved");
    commit_file(root, "later.txt", "later\n");
    let before = checkout(root);

    let (outcome, _session) = resolve_from_inbox(&mut orchestrator);

    assert_eq!(outcome, LedgerOutcome::Applied);
    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::Completed);
    assert!(stage.merged);
    let parents = git(root, &["rev-list", "--parents", "-n1", "main"]);
    assert_eq!(parents.split_whitespace().count(), 3, "a two-parent merge");
    assert_eq!(git(root, &["show", "main:later.txt"]), "later");
    assert_eq!(git(root, &["show", "main:seed.txt"]), "resolved");
    assert_advanced_only(root, &before, &["seed.txt"]);
    assert_eq!(text_of(root, "later.txt"), "later\n");
}

#[test]
fn a_conflict_that_became_clean_lands_without_a_resolver() {
    let repo = repo_with_worktree();
    let root = repo.path();
    make_conflict(root);
    let mut orchestrator = orchestrator_with_completed_stage(root);
    assert!(!orchestrator.try_auto_merge(ID));
    // The operator lands the very change the branch makes.
    commit_file(root, "seed.txt", "stage side");
    let before = checkout(root);
    let work_dir = orchestrator.config.work_dir.clone();

    assert_eq!(orchestrator.spawn_merge_resolution_sessions().unwrap(), 0);

    let stage = on_disk(&orchestrator);
    assert_eq!(stage.status, StageStatus::Completed);
    assert!(stage.merged);
    assert_eq!(merge_resolver_attempts(&work_dir, ID), 0);
    assert!(orchestrator.active_sessions.is_empty());
    assert!(!worktree(root).exists(), "no resolver runs in it");
    assert_advanced_only(root, &before, &[]);
}
