//! `refuse_unreviewed_move` judges every target the record holds against its
//! current tip: a hold the operator has resolved no longer refuses, and a
//! target the refs file does not list still does.

use super::refuse_unreviewed_move;
use crate::git::target_guard::test_support::{commit_file, git, held_repo, repo};
use crate::git::target_guard::{check, RECORD_FILE, REFS_FILE};

#[test]
fn refusal_clears_once_the_ref_is_restored_to_the_accepted_tip() {
    let (repo, accepted, moved) = held_repo(true);
    let message = refuse_unreviewed_move(&repo.root).unwrap_err().to_string();
    assert!(
        message.contains("has a move loom did not accept"),
        "{message}"
    );
    let record = std::fs::read(repo.work.join(RECORD_FILE)).unwrap();

    git(
        &repo.root,
        &["update-ref", "refs/heads/main", &accepted, &moved],
    );

    refuse_unreviewed_move(&repo.root).unwrap();
    assert_eq!(
        std::fs::read(repo.work.join(RECORD_FILE)).unwrap(),
        record,
        "the refusal check must not rewrite the record"
    );
}

#[test]
fn refusal_reads_the_record_not_the_refs_file() {
    let (repo, _accepted, _moved) = held_repo(false);
    std::fs::remove_file(repo.work.join(REFS_FILE)).unwrap();

    let message = refuse_unreviewed_move(&repo.root).unwrap_err().to_string();

    assert!(
        message.contains("the target branch main has a move loom did not accept"),
        "{message}"
    );
}

#[test]
fn refusal_names_a_held_target_after_a_clear_one() {
    let repo = repo();
    check(&repo.root, &repo.work, "main").unwrap();
    git(&repo.root, &["branch", "release"]);
    check(&repo.root, &repo.work, "release").unwrap();
    git(&repo.root, &["checkout", "-q", "release"]);
    commit_file(&repo.root, ".claude/settings.json", "{}\n");
    git(&repo.root, &["checkout", "-q", "main"]);

    let message = refuse_unreviewed_move(&repo.root).unwrap_err().to_string();

    assert!(
        message.contains("the target branch release has a move loom did not accept"),
        "{message}"
    );
}
