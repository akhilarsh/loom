use super::*;
use crate::git::merge::test_support::{
    commit_count, commit_file, git_ok, git_out, init_repo, rev, sibling_dir, stage_branch,
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

/// The merge of a branch adding `b.txt` into `main`, not yet committed.
fn pending_merge(root: &Path) -> PendingMerge<'_> {
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let (old, branch) = (rev(root, "main"), rev(root, "loom/s1"));
    let TreeMerge::Clean { tree } = merge_tree(root, &old, &branch).unwrap() else {
        panic!("expected a clean merge");
    };
    PendingMerge::new(root, &tree, [&old, &branch], "m")
}

#[test]
fn pending_merge_writes_its_commit_once_and_only_when_asked() {
    let repo = init_repo();
    let root = repo.path();
    let mut pending = pending_merge(root);
    let before = commit_count(root);

    let first = pending.commit().unwrap();
    let second = pending.commit().unwrap();

    assert_eq!(first, second);
    assert_eq!(commit_count(root), before + 1);
    assert_eq!(rev(root, &format!("{first}^1")), pending.old());
}

#[test]
fn advance_with_a_stale_old_value_is_blocked_when_updating_a_ref() {
    let repo = init_repo();
    let root = repo.path();
    let mut pending = pending_merge(root);
    git_ok(root, &["checkout", "-b", "other"]);
    let new = pending.commit().unwrap();
    git_ok(root, &["branch", "-f", "main", &new]);

    let advance = advance_target(root, "main", "s1", &mut pending).unwrap();

    assert_eq!(advance, Advance::Blocked(MergeBlock::TargetMoved));
    assert_eq!(rev(root, "main"), new);
}

#[test]
fn advance_with_a_stale_old_value_is_blocked_in_the_checkout() {
    let repo = init_repo();
    let root = repo.path();
    let mut pending = pending_merge(root);
    commit_file(root, "c.txt", "moved", "main moved");
    let moved = rev(root, "main");
    let before = commit_count(root);

    let advance = advance_target(root, "main", "s1", &mut pending).unwrap();

    assert_eq!(advance, Advance::Blocked(MergeBlock::TargetMoved));
    assert_eq!(rev(root, "main"), moved);
    assert!(!root.join("b.txt").exists());
    assert_eq!(
        commit_count(root),
        before,
        "a blocked advance writes no commit"
    );
}

#[test]
fn advance_updates_the_ref_and_logs_the_stage() {
    let repo = init_repo();
    let root = repo.path();
    let mut pending = pending_merge(root);
    git_ok(root, &["checkout", "-b", "other"]);

    let advance = advance_target(root, "main", "s1", &mut pending).unwrap();

    assert_eq!(advance, Advance::Advanced { stash: None });
    assert_eq!(rev(root, "main"), pending.commit().unwrap());
    let reflog = git_out(root, &["reflog", "show", "main", "-1", "--format=%gs"]);
    assert_eq!(reflog, "loom: merge loom/s1");
}

#[test]
fn advance_is_blocked_when_another_worktree_has_the_target() {
    let repo = init_repo();
    let root = repo.path();
    let mut pending = pending_merge(root);
    git_ok(root, &["checkout", "-b", "other"]);
    let (_keep, second) = sibling_dir();
    git_ok(root, &["worktree", "add", second.to_str().unwrap(), "main"]);
    let before = commit_count(root);

    let advance = advance_target(root, "main", "s1", &mut pending).unwrap();

    assert!(matches!(
        advance,
        Advance::Blocked(MergeBlock::TargetCheckedOutElsewhere { .. })
    ));
    assert_eq!(rev(root, "main"), pending.old());
    assert_eq!(
        commit_count(root),
        before,
        "a blocked advance writes no commit"
    );
}

#[test]
fn a_deleted_worktree_does_not_hold_the_target() {
    let repo = init_repo();
    let root = repo.path();
    let mut pending = pending_merge(root);
    git_ok(root, &["checkout", "-b", "other"]);
    let (_keep, second) = sibling_dir();
    git_ok(root, &["worktree", "add", second.to_str().unwrap(), "main"]);
    std::fs::remove_dir_all(&second).unwrap();

    let advance = advance_target(root, "main", "s1", &mut pending).unwrap();

    assert_eq!(advance, Advance::Advanced { stash: None });
    assert_eq!(rev(root, "main"), pending.commit().unwrap());
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

#[test]
fn the_new_blocks_serialize_and_explain_themselves() {
    let refused = MergeBlock::FastForwardRefused {
        detail: "odd".to_string(),
    };
    let not_restored = MergeBlock::StashNotRestored {
        backup_ref: "refs/loom/autostash/s1-1".to_string(),
    };
    for block in [&refused, &not_restored] {
        let json = serde_json::to_string(block).unwrap();
        assert_eq!(&serde_json::from_str::<MergeBlock>(&json).unwrap(), block);
    }
    assert!(refused.to_string().contains("odd"));
    let text = not_restored.to_string();
    assert!(text.contains("git stash pop") && text.contains("refs/loom/autostash/s1-1"));
}

#[test]
fn stash_reapply_notice_says_whether_the_merge_landed_without_the_changes() {
    let restored = StashReapply {
        backup_ref: "refs/loom/autostash/s1-1".to_string(),
        restored: true,
    };
    let lost = StashReapply {
        restored: false,
        ..restored.clone()
    };
    assert!(restored.notice().contains("reapplied"));
    assert!(lost.notice().contains("LANDED") && lost.notice().contains("git stash pop"));
}
