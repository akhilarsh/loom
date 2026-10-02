use super::*;
use crate::git::merge::test_support::{commit_file, git_ok, init_repo, rev, stage_branch};

fn overlap(paths: &[&str]) -> MergeBlock {
    MergeBlock::UncommittedOverlap {
        paths: paths.iter().map(|p| p.to_string()).collect(),
    }
}

#[test]
fn git_messages_that_list_paths_become_an_overlap() {
    let cases = [
        (
            "error: Your local changes to the following files would be overwritten by merge:\n\ta\n\tb c\nPlease commit your changes or stash them before you merge.\nAborting\n",
            overlap(&["a", "b c"]),
        ),
        (
            "error: The following untracked working tree files would be overwritten by merge:\n\tb\nPlease move or remove them before you merge.\nAborting\n",
            overlap(&["b"]),
        ),
        (
            "error: The following untracked working tree files would be removed by merge:\n\tt\nPlease move or remove them before you merge.\nAborting\n",
            overlap(&["t"]),
        ),
        (
            "error: Updating the following directories would lose untracked files in them:\n\td\n\nAborting\n",
            overlap(&["d"]),
        ),
    ];
    for (stderr, expected) in cases {
        assert_eq!(refused_block(stderr), expected, "{stderr}");
    }
}

#[test]
fn a_message_without_paths_becomes_a_one_line_detail() {
    let block =
        refused_block("hint: Diverging\nhint:\nfatal: Not possible to fast-forward, aborting.\n");
    assert_eq!(
        block,
        MergeBlock::FastForwardRefused {
            detail: "hint: Diverging hint: fatal: Not possible to fast-forward, aborting."
                .to_string()
        }
    );

    let MergeBlock::FastForwardRefused { detail } = refused_block(&"word ".repeat(200)) else {
        panic!("expected FastForwardRefused");
    };
    assert_eq!(detail.chars().count(), DETAIL_LIMIT);
}

#[test]
fn a_refusal_naming_paths_is_an_overlap_and_changes_nothing() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let (old, new) = (rev(root, "main"), rev(root, "loom/s1"));
    std::fs::write(root.join("b.txt"), "mine").unwrap();

    let advance = fast_forward(root, (&old, &new), &[], None).unwrap();

    assert_eq!(advance, Advance::Blocked(overlap(&["b.txt"])));
    assert_eq!(rev(root, "main"), old);
    assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "mine");
}

#[test]
fn a_refusal_after_the_target_moved_is_target_moved() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let (old, new) = (rev(root, "main"), rev(root, "loom/s1"));
    commit_file(root, "c.txt", "moved", "main moves");

    let advance = fast_forward(root, (&old, &new), &[], None).unwrap();

    assert_eq!(advance, Advance::Blocked(MergeBlock::TargetMoved));
}

#[test]
fn a_refusal_without_paths_is_fast_forward_refused() {
    let repo = init_repo();
    let root = repo.path();
    stage_branch(root, "s1", &[("b.txt", "branch")]);
    let new = rev(root, "loom/s1");
    commit_file(root, "c.txt", "diverges", "main diverges");
    let old = rev(root, "main");

    let advance = fast_forward(root, (&old, &new), &[], None).unwrap();

    let Advance::Blocked(MergeBlock::FastForwardRefused { detail }) = advance else {
        panic!("expected FastForwardRefused, got {advance:?}");
    };
    assert!(detail.contains("fast-forward"), "{detail}");
    git_ok(root, &["status", "--porcelain"]);
}
