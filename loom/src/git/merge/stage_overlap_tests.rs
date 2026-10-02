//! `merge_stage` where the operator's checkout holds ignored or conflicting
//! paths, and where the merge must write no commit, resolve branches (not
//! tags) or refuse an operator operation.

use super::test_support::{
    commit_count, git_ok, git_out, init_repo, isolated_git, lock_dir, rev, sibling_dir, snapshot,
};
use super::*;

fn merge(root: &Path, work: &tempfile::TempDir) -> MergeResult {
    merge_stage("s1", "main", root, work.path()).unwrap()
}

/// `loom/s1` from `main` with `files` committed (parents created, ignore
/// rules overridden), then back on `main`.
fn branch_with(root: &Path, files: &[(&str, &str)]) {
    git_ok(root, &["checkout", "-b", "loom/s1", "main"]);
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        git_ok(root, &["add", "-f", name]);
    }
    git_ok(root, &["commit", "-m", "stage"]);
    git_ok(root, &["checkout", "main"]);
}

fn ignore(root: &Path, pattern: &str) {
    std::fs::write(root.join(".git/info/exclude"), format!("{pattern}\n")).unwrap();
}

fn assert_blocked_overlap(result: MergeResult, expected: &[&str]) {
    match result {
        MergeResult::Blocked(MergeBlock::UncommittedOverlap { paths }) => {
            assert_eq!(paths, expected);
        }
        other => panic!("expected UncommittedOverlap, got {other:?}"),
    }
}

#[test]
fn an_ignored_file_the_branch_adds_blocks_and_keeps_its_bytes() {
    let repo = init_repo();
    let root = repo.path();
    ignore(root, "ign.txt");
    branch_with(root, &[("ign.txt", "branch")]);
    std::fs::write(root.join("ign.txt"), "mine").unwrap();
    let (work, head, commits) = (lock_dir(), rev(root, "main"), commit_count(root));

    assert_blocked_overlap(merge(root, &work), &["ign.txt"]);

    assert_eq!(
        std::fs::read_to_string(root.join("ign.txt")).unwrap(),
        "mine"
    );
    assert_eq!(rev(root, "main"), head);
    assert_eq!(commit_count(root), commits);
}

#[test]
fn a_byte_identical_ignored_file_is_replaced_by_the_merge() {
    let repo = init_repo();
    let root = repo.path();
    ignore(root, "ign.txt");
    branch_with(root, &[("ign.txt", "branch")]);
    std::fs::write(root.join("ign.txt"), "branch").unwrap();
    let work = lock_dir();

    assert!(matches!(merge(root, &work), MergeResult::Success { .. }));

    assert_eq!(
        std::fs::read_to_string(root.join("ign.txt")).unwrap(),
        "branch"
    );
    assert_eq!(
        git_out(root, &["ls-tree", "--name-only", "main", "ign.txt"]),
        "ign.txt"
    );
}

#[test]
fn an_ignored_file_where_the_branch_needs_a_directory_blocks() {
    let repo = init_repo();
    let root = repo.path();
    ignore(root, "dir");
    branch_with(root, &[("dir/x.txt", "branch")]);
    std::fs::write(root.join("dir"), "mine").unwrap();
    let work = lock_dir();

    assert_blocked_overlap(merge(root, &work), &["dir"]);

    assert_eq!(std::fs::read_to_string(root.join("dir")).unwrap(), "mine");
}

#[test]
fn an_untracked_file_below_a_path_the_branch_adds_as_a_file_blocks() {
    let repo = init_repo();
    let root = repo.path();
    branch_with(root, &[("foo", "file")]);
    std::fs::create_dir(root.join("foo")).unwrap();
    std::fs::write(root.join("foo/bar"), "mine").unwrap();
    let (work, before) = (lock_dir(), snapshot(root));

    assert_blocked_overlap(merge(root, &work), &["foo", "foo/bar"]);

    assert_eq!(snapshot(root), before);
}

#[test]
fn an_untracked_file_where_the_branch_adds_a_directory_blocks() {
    let repo = init_repo();
    let root = repo.path();
    branch_with(root, &[("foo/bar", "file")]);
    std::fs::write(root.join("foo"), "mine").unwrap();
    let (work, before) = (lock_dir(), snapshot(root));

    assert_blocked_overlap(merge(root, &work), &["foo"]);

    assert_eq!(snapshot(root), before);
}

#[test]
fn a_blocked_attempt_writes_no_commit() {
    let repo = init_repo();
    let root = repo.path();
    branch_with(root, &[("b.txt", "branch")]);
    let work = lock_dir();

    // The target is checked out in another worktree.
    git_ok(root, &["checkout", "-b", "other"]);
    let (_keep, second) = sibling_dir();
    git_ok(root, &["worktree", "add", second.to_str().unwrap(), "main"]);
    let commits = commit_count(root);
    assert!(matches!(
        merge(root, &work),
        MergeResult::Blocked(MergeBlock::TargetCheckedOutElsewhere { .. })
    ));
    assert_eq!(commit_count(root), commits);
    git_ok(root, &["worktree", "remove", second.to_str().unwrap()]);

    // An untracked file with different bytes sits on a path the branch adds.
    git_ok(root, &["checkout", "main"]);
    std::fs::write(root.join("b.txt"), "mine").unwrap();
    assert!(matches!(
        merge(root, &work),
        MergeResult::Blocked(MergeBlock::UncommittedOverlap { .. })
    ));
    assert_eq!(commit_count(root), commits);
}

#[test]
fn an_unmerged_entry_blocks_without_a_commit() {
    let repo = init_repo();
    let root = repo.path();
    branch_with(root, &[("b.txt", "branch")]);
    std::fs::write(root.join("a.txt"), "stashed").unwrap();
    git_ok(root, &["stash"]);
    std::fs::write(root.join("a.txt"), "committed").unwrap();
    git_ok(root, &["commit", "-am", "main moves"]);
    // The pop conflicts and leaves an unmerged index entry.
    assert!(!isolated_git(root, &["stash", "pop"]).status.success());
    let (work, commits) = (lock_dir(), commit_count(root));

    assert_blocked_overlap(merge(root, &work), &["a.txt"]);

    assert_eq!(commit_count(root), commits);
}

#[test]
fn tags_named_like_the_branches_do_not_win() {
    let repo = init_repo();
    let root = repo.path();
    branch_with(root, &[("b.txt", "branch")]);
    let seed = rev(root, "main");
    std::fs::write(root.join("c.txt"), "main").unwrap();
    git_ok(root, &["add", "c.txt"]);
    git_ok(root, &["commit", "-m", "main moves"]);
    let tip = rev(root, "main");
    git_ok(root, &["tag", "main", &seed]);
    git_ok(root, &["tag", "loom/s1", &seed]);
    let work = lock_dir();

    assert!(matches!(merge(root, &work), MergeResult::Success { .. }));

    assert_eq!(rev(root, "refs/heads/main^1"), tip);
    assert_eq!(
        git_out(
            root,
            &["ls-tree", "--name-only", "refs/heads/main", "b.txt"]
        ),
        "b.txt"
    );
}

#[test]
fn a_sequencer_directory_blocks_the_merge() {
    let repo = init_repo();
    let root = repo.path();
    branch_with(root, &[("b.txt", "branch")]);
    std::fs::create_dir(root.join(".git/sequencer")).unwrap();
    let work = lock_dir();

    match merge(root, &work) {
        MergeResult::Blocked(MergeBlock::OperatorOperation { marker }) => {
            assert_eq!(marker, "sequencer");
        }
        other => panic!("expected OperatorOperation, got {other:?}"),
    }
}

#[test]
fn the_reported_stats_match_the_landed_merge() {
    let repo = init_repo();
    let root = repo.path();
    branch_with(root, &[("b.txt", "one\ntwo\n"), ("c.txt", "three\n")]);
    let work = lock_dir();

    let MergeResult::Success {
        files_changed,
        insertions,
        deletions,
        ..
    } = merge(root, &work)
    else {
        panic!("expected a landed merge");
    };

    let stat = git_out(root, &["diff", "--shortstat", "main^1", "main"]);
    assert_eq!(
        (files_changed, insertions, deletions),
        parse_merge_stats(&stat)
    );
    assert_eq!((files_changed, insertions, deletions), (2, 3, 0));
}
