//! `loom target status` for a target branch that does not resolve and for a
//! hold the operator has since resolved.

use super::tests::report_of;
use crate::git::target_guard::test_support::{git, held_repo, recorded, Repo};
use crate::git::target_guard::RECORD_FILE;

/// Detach HEAD, then delete `main`: the branch the guard recorded is gone.
fn delete_main(repo: &Repo) {
    git(&repo.root, &["checkout", "-q", "--detach"]);
    git(&repo.root, &["branch", "-q", "-D", "main"]);
}

#[test]
fn report_of_a_deleted_target_branch_names_both_ways_out() {
    let (repo, accepted) = recorded();
    delete_main(&repo);

    let text = report_of(&repo);

    assert!(text.contains("Target: main"), "{text}");
    assert!(
        text.contains("State: refs/heads/main does not resolve"),
        "{text}"
    );
    assert!(text.contains(&format!("Accepted: {accepted}")), "{text}");
    assert!(
        text.contains(&format!(
            "Restore: git update-ref refs/heads/main {accepted}"
        )),
        "{text}"
    );
    let record = repo.work.join(RECORD_FILE).display().to_string();
    assert!(
        text.contains(&format!("remove {record} once you have reviewed")),
        "{text}"
    );
}

#[test]
fn report_of_a_deleted_target_branch_and_an_unreadable_record_names_the_record() {
    let (repo, _accepted) = recorded();
    delete_main(&repo);
    std::fs::write(repo.work.join(RECORD_FILE), "{ not json").unwrap();

    let text = report_of(&repo);

    assert!(text.contains("does not resolve"), "{text}");
    assert!(text.contains("is unreadable"), "{text}");
    assert!(!text.contains("Restore:"), "{text}");
}

#[test]
fn report_of_a_restored_target_is_in_sync_whatever_hold_the_record_carries() {
    let (repo, accepted, moved) = held_repo(true);
    git(
        &repo.root,
        &["update-ref", "refs/heads/main", &accepted, &moved],
    );

    let text = report_of(&repo);

    assert!(text.contains("State: in sync"), "{text}");
    assert!(!text.contains("HELD"), "{text}");
}
