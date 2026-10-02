//! The memo of a recorded hold: a hold of the observed tip is returned
//! without evaluating the move again, unless it could not be evaluated.

use super::test_support::{activate, git, repo};
use super::tests::{commit, guard, held, record};
use super::*;

#[test]
fn an_unevaluable_hold_is_evaluated_again_once_its_cause_is_repaired() {
    let repo = repo();
    activate(&repo.root);
    record(&repo);
    let ledger = repo.work.join(LEDGER_FILE);
    std::fs::remove_file(&ledger).unwrap();
    std::fs::create_dir(&ledger).unwrap();
    let k = commit(&repo.root, "doc/loom/knowledge/k.md");
    let hold = held(guard(&repo));
    assert!(
        matches!(&hold.reasons[..], [HoldReason::Unevaluable { .. }]),
        "{hold:?}"
    );
    assert_eq!(recorded_hold(&repo.work, "main").unwrap(), Some(hold));

    std::fs::remove_dir(&ledger).unwrap();

    assert_eq!(guard(&repo), GuardState::Clear { accepted: k });
    assert_eq!(recorded_hold(&repo.work, "main").unwrap(), None);
}

#[test]
fn a_held_evaluable_tip_is_returned_from_the_memo_without_evaluating_it_again() {
    let repo = repo();
    let a = record(&repo);
    let b = commit(&repo.root, ".claude/settings.json");
    let hold = held(guard(&repo));
    assert!(
        matches!(&hold.reasons[..], [HoldReason::ControlPaths { .. }]),
        "{hold:?}"
    );
    git(&repo.root, &["branch", "loom/s", &b]);
    let scope = Scope {
        repo_root: &repo.root,
        work_dir: &repo.work,
        key: "main",
    };
    let fresh = evaluate(&scope, &a, &b, false);
    assert!(
        fresh.contains(&HoldReason::StageWork {
            branches: vec!["loom/s".to_string()]
        }),
        "an evaluation now must differ from the memo: {fresh:?}"
    );

    assert_eq!(held(guard(&repo)), hold);
}
