use super::*;
use crate::git::merge::test_support::{
    commit_file, git_ok, git_out, init_repo, isolated_git, sibling_dir, stage_branch,
};

fn inputs(root: &Path) -> u64 {
    blocked_merge_inputs(root, "s1", "main", &[]).unwrap()
}

fn watching(root: &Path, path: &str) -> u64 {
    blocked_merge_inputs(root, "s1", "main", &[path.to_string()]).unwrap()
}

fn append(root: &Path, name: &str, text: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(root.join(name))
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

/// `main` and `other` both change `a.txt`, so merging one into the other
/// conflicts. `main` is checked out afterwards.
fn conflicting_branches(root: &Path) {
    git_ok(root, &["checkout", "-q", "-b", "other"]);
    commit_file(root, "a.txt", "other side", "other");
    git_ok(root, &["checkout", "-q", "main"]);
    commit_file(root, "a.txt", "main side", "main");
}

#[test]
fn the_same_state_hashes_equal_twice() {
    let repo = init_repo();
    stage_branch(repo.path(), "s1", &[("b.txt", "branch")]);

    assert_eq!(inputs(repo.path()), inputs(repo.path()));
}

#[test]
fn a_new_commit_on_the_target_changes_the_inputs() {
    let repo = init_repo();
    stage_branch(repo.path(), "s1", &[("b.txt", "branch")]);
    let before = inputs(repo.path());

    commit_file(repo.path(), "c.txt", "main", "main");

    assert_ne!(inputs(repo.path()), before);
}

#[test]
fn a_new_commit_on_the_stage_branch_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let before = inputs(root);

    git_ok(root, &["checkout", "-q", "loom/s1"]);
    commit_file(root, "d.txt", "more", "more");
    git_ok(root, &["checkout", "-q", "main"]);

    assert_ne!(inputs(root), before);
}

#[test]
fn growing_an_already_modified_tracked_file_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    append(root, "a.txt", "one");
    let before = inputs(root);
    assert_eq!(git_out(root, &["status", "--porcelain"]), "M a.txt");

    append(root, "a.txt", "two");

    assert_eq!(git_out(root, &["status", "--porcelain"]), "M a.txt");
    assert_ne!(inputs(root), before);
}

#[test]
fn a_new_untracked_file_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let before = inputs(root);

    std::fs::write(root.join("new.txt"), "x").unwrap();

    assert_ne!(inputs(root), before);
}

#[test]
fn checking_the_target_out_in_a_second_worktree_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    git_ok(root, &["checkout", "-q", "-b", "other"]);
    let before = inputs(root);

    let (_keep, second) = sibling_dir();
    git_ok(
        root,
        &["worktree", "add", "-q", second.to_str().unwrap(), "main"],
    );

    assert_ne!(inputs(root), before);
}

#[test]
fn a_cherry_pick_in_progress_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    conflicting_branches(root);
    let before = inputs(root);

    assert!(!isolated_git(root, &["cherry-pick", "other"])
        .status
        .success());

    assert!(root.join(".git/CHERRY_PICK_HEAD").exists());
    assert_ne!(inputs(root), before);
}

#[test]
fn a_merge_in_progress_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    conflicting_branches(root);
    let before = inputs(root);

    assert!(!isolated_git(root, &["merge", "other"]).status.success());

    assert!(root.join(".git/MERGE_HEAD").exists());
    assert_ne!(inputs(root), before);
}

#[test]
fn computing_the_inputs_writes_no_object() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    append(root, "a.txt", "edit");
    std::fs::write(root.join("new.txt"), "x").unwrap();
    let before = git_out(root, &["count-objects", "-v"]);

    inputs(root);
    inputs(root);

    assert_eq!(git_out(root, &["count-objects", "-v"]), before);
}

#[test]
fn a_watched_path_that_is_deleted_created_or_resized_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    std::fs::write(root.join(".gitignore"), "ign.txt\n").unwrap();
    std::fs::write(root.join("ign.txt"), "one").unwrap();
    let present = watching(root, "ign.txt");
    assert_eq!(watching(root, "ign.txt"), present);
    assert_eq!(git_out(root, &["status", "--porcelain"]), "?? .gitignore");

    std::fs::write(root.join("ign.txt"), "longer").unwrap();
    let resized = watching(root, "ign.txt");
    assert_ne!(resized, present);

    std::fs::remove_file(root.join("ign.txt")).unwrap();
    let deleted = watching(root, "ign.txt");
    assert_ne!(deleted, resized);

    std::fs::write(root.join("ign.txt"), "one").unwrap();
    assert_ne!(watching(root, "ign.txt"), deleted);
}

#[test]
fn computing_the_inputs_takes_no_index_lock() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    // Stale stat data makes a locking `git status` rewrite the index.
    std::fs::write(root.join("a.txt"), "seed").unwrap();
    let index = root.join(".git/index");
    let before = std::fs::metadata(&index).unwrap().modified().unwrap();

    inputs(root);

    assert_eq!(
        std::fs::metadata(&index).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn an_index_lock_appearing_changes_the_inputs() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let before = inputs(root);

    std::fs::write(root.join(".git/index.lock"), "").unwrap();
    let locked = inputs(root);
    assert_ne!(locked, before);

    std::fs::remove_file(root.join(".git/index.lock")).unwrap();
    assert_eq!(inputs(root), before);
}
