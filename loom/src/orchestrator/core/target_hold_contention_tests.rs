//! The daemon's guard check when it cannot get a verdict under the merge
//! lock: a lock another owner holds gets the target judged without it, and
//! an error holds the target until a later check answers. Either way the
//! gates a hold closes stay closed for a move nobody judged clear.

use std::time::Duration;

use crate::git::merge::lock::MergeLock;
use crate::git::target_guard::{accepted_tip, Hold, HoldReason};
use crate::git::MergeBlock;
use crate::models::stage::{Stage, StageStatus, StageType};
use crate::orchestrator::core::merge_resolver_attempts;
use crate::orchestrator::scheduling_report::BlockReason;
use crate::verify::transitions::{load_stage, save_stage};

use super::tests::{commit, git, graph, held, orchestrator, repo, stage_branch, tip, ID};

/// Run `check` while the merge lock is held by another owner.
fn with_lock_taken<T>(work_dir: &std::path::Path, check: impl FnOnce() -> T) -> T {
    let lock = MergeLock::acquire(work_dir, Duration::from_secs(5)).unwrap();
    let result = check();
    lock.release().unwrap();
    result
}

#[test]
fn a_contended_check_holds_a_move_and_keeps_resolvers_and_knowledge_stages_waiting() {
    let repo = repo();
    let root = repo.path();
    let mut orchestrator = orchestrator(root, graph(&[]));
    assert_eq!(orchestrator.check_target_guard(), None);
    let accepted = tip(root, "main");
    let moved = commit(root, ".claude/settings.json", "{}\n");
    let work_dir = orchestrator.config.work_dir.clone();

    let hold = with_lock_taken(&work_dir, || orchestrator.check_target_guard());

    let hold = hold.expect("a contended check let an unjudged move through");
    assert_eq!(
        (hold.accepted.clone(), hold.observed.clone()),
        (accepted, moved)
    );
    assert!(orchestrator.target_held());
    assert!(
        !orchestrator.target_hold_announced,
        "a lockless hold was printed"
    );
    let conflicted = Stage {
        id: ID.to_string(),
        status: StageStatus::MergeConflict,
        ..Stage::default()
    };
    save_stage(&conflicted, &work_dir).unwrap();
    assert_eq!(orchestrator.spawn_merge_resolution_sessions().unwrap(), 0);
    assert_eq!(merge_resolver_attempts(&work_dir, ID), 0);
    let knowledge = Stage {
        stage_type: StageType::Knowledge,
        ..Stage::default()
    };
    let reason = orchestrator.spawn_hold_reason(&knowledge);
    assert_eq!(reason, Some(BlockReason::TargetHeld));

    let locked = orchestrator
        .check_target_guard()
        .expect("the lock freed the move");
    assert_eq!(
        (locked.observed, locked.reasons),
        (hold.observed, hold.reasons)
    );
    assert!(
        orchestrator.target_hold_announced,
        "the locked check did not print"
    );
}

#[test]
fn a_contended_check_clears_a_move_the_guard_would_accept() {
    let repo = repo();
    let root = repo.path();
    let mut orchestrator = orchestrator(root, graph(&[]));
    assert_eq!(orchestrator.check_target_guard(), None);
    let accepted = tip(root, "main");
    let advanced = commit(root, "plain.txt", "operator work\n");
    let work_dir = orchestrator.config.work_dir.clone();

    let contended = with_lock_taken(&work_dir, || orchestrator.check_target_guard());

    assert_eq!(contended, None);
    assert!(!orchestrator.target_held());
    let unwritten = accepted_tip(&work_dir, "main").unwrap();
    assert_eq!(
        unwritten,
        Some(accepted),
        "a contended check wrote the record"
    );
    assert_eq!(orchestrator.check_target_guard(), None);
    assert_eq!(accepted_tip(&work_dir, "main").unwrap(), Some(advanced));
}

#[test]
fn the_auto_merge_precheck_holds_a_contended_move() {
    let repo = repo();
    let root = repo.path();
    let work = stage_branch(root);
    let mut orchestrator = orchestrator(root, graph(&[]));
    assert_eq!(orchestrator.check_target_guard(), None);
    let accepted = tip(root, "main");
    let moved = commit(root, ".claude/settings.json", "{}\n");
    let work_dir = orchestrator.config.work_dir.clone();
    let completed = Stage {
        id: ID.to_string(),
        status: StageStatus::Completed,
        completed_commit: Some(work),
        ..Stage::default()
    };
    save_stage(&completed, &work_dir).unwrap();

    let merged = with_lock_taken(&work_dir, || orchestrator.try_auto_merge(ID));

    assert!(!merged, "a contended precheck merged into a moved target");
    let block = MergeBlock::TargetHeld {
        target: "main".to_string(),
        accepted,
        observed: moved,
    };
    assert_eq!(load_stage(ID, &work_dir).unwrap().merge.block, Some(block));
}

#[test]
fn a_check_error_holds_the_target_until_a_check_answers() {
    let repo = repo();
    let root = repo.path();
    let mut orchestrator = orchestrator(root, graph(&[]));
    assert_eq!(orchestrator.check_target_guard(), None);
    let accepted = tip(root, "main");
    git(root, &["update-ref", "-d", "refs/heads/main"]);

    let hold = orchestrator
        .check_target_guard()
        .expect("an erroring check left the target clear");

    assert!(orchestrator.target_held());
    assert_eq!(
        (hold.accepted.as_str(), hold.observed.as_str()),
        (accepted.as_str(), "")
    );
    let unevaluable = matches!(hold.reasons[..], [HoldReason::Unevaluable { .. }]);
    assert!(unevaluable, "{hold:?}");
    let logged = orchestrator.target_guard_error.clone();
    assert!(logged.is_some(), "the error was not logged");
    assert_eq!(orchestrator.check_target_guard(), Some(hold));
    assert_eq!(orchestrator.target_guard_error, logged);

    git(root, &["update-ref", "refs/heads/main", &accepted]);
    assert_eq!(orchestrator.check_target_guard(), None);
    assert!(!orchestrator.target_held());
    assert_eq!(orchestrator.target_guard_error, None);
}

#[test]
fn the_auto_merge_precheck_holds_while_the_check_errors() {
    let repo = repo();
    let root = repo.path();
    let work = stage_branch(root);
    let mut orchestrator = orchestrator(root, graph(&[]));
    assert_eq!(orchestrator.check_target_guard(), None);
    let accepted = tip(root, "main");
    let work_dir = orchestrator.config.work_dir.clone();
    let completed = Stage {
        id: ID.to_string(),
        status: StageStatus::Completed,
        completed_commit: Some(work),
        ..Stage::default()
    };
    save_stage(&completed, &work_dir).unwrap();
    git(root, &["update-ref", "-d", "refs/heads/main"]);

    assert!(!orchestrator.try_auto_merge(ID));

    let block = MergeBlock::TargetHeld {
        target: "main".to_string(),
        accepted,
        observed: String::new(),
    };
    assert_eq!(load_stage(ID, &work_dir).unwrap().merge.block, Some(block));
}

#[test]
fn a_repeated_guard_error_is_logged_once_until_a_check_answers() {
    let repo = repo();
    let mut orchestrator = orchestrator(repo.path(), graph(&[]));
    let error = anyhow::anyhow!("the guard record does not parse");

    assert!(orchestrator.fresh_guard_error(&error).is_some());
    assert_eq!(orchestrator.fresh_guard_error(&error), None);
    let other = anyhow::anyhow!("another failure");
    assert!(orchestrator.fresh_guard_error(&other).is_some());
    assert_eq!(orchestrator.check_target_guard(), None);
    assert!(orchestrator.fresh_guard_error(&other).is_some());
}

#[test]
fn a_hold_is_printed_again_when_its_reasons_change() {
    let repo = repo();
    let (orchestrator, _, _) = held(repo.path(), graph(&[]));
    let printed = orchestrator.target_hold.clone().expect("the hold is kept");
    assert!(orchestrator.target_hold_announced);

    let other_reasons = Hold {
        reasons: vec![HoldReason::NotFastForward],
        ..printed.clone()
    };
    let other_tip = Hold {
        observed: "0".repeat(40),
        ..printed.clone()
    };

    assert!(!orchestrator.hold_is_news(&printed));
    assert!(orchestrator.hold_is_news(&other_reasons));
    assert!(orchestrator.hold_is_news(&other_tip));
}
