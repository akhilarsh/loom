use super::*;
use crate::git::merge::test_support::{
    commit_file, git_ok, git_out, init_repo, rev, sibling_dir, stage_branch,
};

#[test]
fn merge_tree_clean_returns_the_merged_tree() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    commit_file(root, "c.txt", "main", "main");

    let merged = merge_tree(root, "main", "loom/s1").unwrap();

    let TreeMerge::Clean { tree } = merged else {
        panic!("expected a clean merge, got {merged:?}");
    };
    let names = git_out(root, &["ls-tree", "--name-only", &tree]);
    assert_eq!(names, "a.txt\nb.txt\nc.txt");
}

#[test]
fn merge_tree_conflict_lists_each_path_once() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("a.txt", "branch")]);
    commit_file(root, "a.txt", "main", "main");

    let merged = merge_tree(root, "main", "loom/s1").unwrap();

    assert_eq!(
        merged,
        TreeMerge::Conflict {
            paths: vec!["a.txt".to_string()]
        }
    );
}

#[test]
fn merge_tree_reports_git_errors_with_stderr() {
    let repo = init_repo();
    let error = merge_tree(repo.path(), "main", "no-such-branch").unwrap_err();
    assert!(error.to_string().contains("merge-tree"), "{error}");
}

#[test]
fn commit_merge_writes_a_two_parent_commit() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    commit_file(root, "c.txt", "main", "main");
    let (main, branch) = (rev(root, "main"), rev(root, "loom/s1"));
    let TreeMerge::Clean { tree } = merge_tree(root, &main, &branch).unwrap() else {
        panic!("expected a clean merge");
    };

    let commit = commit_merge(root, &tree, [&main, &branch], "Merge test").unwrap();

    let parents = git_out(root, &["rev-list", "--parents", "-n1", &commit]);
    assert_eq!(parents, format!("{commit} {main} {branch}"));
    assert_eq!(
        git_out(root, &["log", "-1", "--format=%s", &commit]),
        "Merge test"
    );
    assert_eq!(rev(root, "main"), main, "commit_merge must not move a ref");
}

/// A merge commit on top of `main` built from a branch, with the checkout
/// moved off `main` so `update-ref` is the mechanism.
fn merge_commit_off_main(root: &Path) -> (String, String) {
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let (old, branch) = (rev(root, "main"), rev(root, "loom/s1"));
    let TreeMerge::Clean { tree } = merge_tree(root, &old, &branch).unwrap() else {
        panic!("expected a clean merge");
    };
    git_ok(root, &["checkout", "-b", "other"]);
    (
        old.clone(),
        commit_merge(root, &tree, [&old, &branch], "m").unwrap(),
    )
}

#[test]
fn advance_with_a_stale_old_value_is_blocked_when_updating_a_ref() {
    let repo = init_repo();
    let root = repo.path();
    let (old, new) = merge_commit_off_main(root);
    git_ok(root, &["branch", "-f", "main", &new]);

    let advance = advance_target(root, "main", &old, &new, "s1").unwrap();

    assert_eq!(advance, Advance::Blocked(MergeBlock::TargetMoved));
    assert_eq!(rev(root, "main"), new);
}

#[test]
fn advance_with_a_stale_old_value_is_blocked_in_the_checkout() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let (old, branch) = (rev(root, "main"), rev(root, "loom/s1"));
    commit_file(root, "c.txt", "moved", "main moved");
    let moved = rev(root, "main");

    let advance = advance_target(root, "main", &old, &branch, "s1").unwrap();

    assert_eq!(advance, Advance::Blocked(MergeBlock::TargetMoved));
    assert_eq!(rev(root, "main"), moved);
    assert!(!root.join("b.txt").exists());
}

#[test]
fn advance_updates_the_ref_and_logs_the_stage() {
    let repo = init_repo();
    let root = repo.path();
    let (old, new) = merge_commit_off_main(root);

    let advance = advance_target(root, "main", &old, &new, "s1").unwrap();

    assert_eq!(advance, Advance::Advanced { backup_ref: None });
    assert_eq!(rev(root, "main"), new);
    let reflog = git_out(root, &["reflog", "show", "main", "-1", "--format=%gs"]);
    assert_eq!(reflog, "loom: merge loom/s1");
}

#[test]
fn advance_is_blocked_when_another_worktree_has_the_target() {
    let repo = init_repo();
    let root = repo.path();
    let (old, new) = merge_commit_off_main(root);
    let (_keep, second) = sibling_dir();
    git_ok(root, &["worktree", "add", second.to_str().unwrap(), "main"]);

    let advance = advance_target(root, "main", &old, &new, "s1").unwrap();

    assert!(matches!(
        advance,
        Advance::Blocked(MergeBlock::TargetCheckedOutElsewhere { .. })
    ));
    assert_eq!(rev(root, "main"), old);
}

#[test]
fn merge_block_serializes_with_a_kind_tag() {
    let block = MergeBlock::UncommittedOverlap {
        paths: vec!["a.txt".to_string()],
    };
    let json = serde_json::to_string(&block).unwrap();
    assert_eq!(json, r#"{"kind":"uncommitted_overlap","paths":["a.txt"]}"#);
    assert_eq!(serde_json::from_str::<MergeBlock>(&json).unwrap(), block);
    assert!(block.to_string().contains("a.txt"));
}
