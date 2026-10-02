//! The evaluation of moves loom did not make: each hold reason, the
//! attestation chain walk over a ledger, and stage work.

use super::test_support::{activate, git, repo, Repo};
use super::tests::{attest, commit, crafted, guard, held, ledger_line, move_main, record, tip};
use super::*;

fn reasons(state: GuardState) -> Vec<HoldReason> {
    held(state).reasons
}

/// `loom/s` from `main` with one commit per file, `main` checked out again;
/// returns the branch's commits.
fn stage_branch(root: &Path, files: &[&str]) -> Vec<String> {
    git(root, &["checkout", "-b", "loom/s"]);
    let commits = files.iter().map(|file| commit(root, file)).collect();
    git(root, &["checkout", "main"]);
    commits
}

#[test]
fn a_rewrite_alone_is_not_a_fast_forward() {
    let repo = repo();
    commit(&repo.root, "second.txt");
    record(&repo);
    git(&repo.root, &["reset", "--hard", "HEAD~1"]);
    commit(&repo.root, "third.txt");

    assert_eq!(reasons(guard(&repo)), vec![HoldReason::NotFastForward]);
}

#[test]
fn a_control_path_alone_is_named() {
    let repo = repo();
    record(&repo);
    commit(&repo.root, ".mcp.json");

    let paths = vec![".mcp.json".to_string()];
    assert_eq!(
        reasons(guard(&repo)),
        vec![HoldReason::ControlPaths { paths }]
    );
}

#[test]
fn an_unattested_change_alone_names_its_range() {
    let repo = repo();
    activate(&repo.root);
    let a = record(&repo);
    let b = commit(&repo.root, "src/b.rs");

    let paths = vec!["src/b.rs".to_string()];
    let expected = HoldReason::Unattested {
        from: a,
        to: b,
        paths,
    };
    assert_eq!(reasons(guard(&repo)), vec![expected]);
}

#[test]
fn a_partial_fast_forward_into_a_stage_branch_is_stage_work() {
    let repo = repo();
    let stage = stage_branch(&repo.root, &["a.txt", "b.txt"]);
    record(&repo);
    move_main(&repo.root, &stage[0]);

    let branches = vec!["loom/s".to_string()];
    assert_eq!(
        reasons(guard(&repo)),
        vec![HoldReason::StageWork { branches }]
    );
}

#[test]
fn an_unreadable_accepted_tip_is_unevaluable_every_time() {
    let repo = repo();
    let missing = "1".repeat(40);
    let state = check_locked(&repo.root, &repo.work, "main", &missing).unwrap();
    assert_eq!(state, GuardState::Clear { accepted: missing });

    for _ in 0..2 {
        let reasons = reasons(guard(&repo));
        assert!(matches!(&reasons[..], [HoldReason::Unevaluable { error }] if !error.is_empty()));
    }
}

#[test]
fn an_unattested_knowledge_only_change_is_accepted() {
    let repo = repo();
    activate(&repo.root);
    record(&repo);
    let b = commit(&repo.root, "doc/loom/knowledge/x.md");

    assert_eq!(guard(&repo), GuardState::Clear { accepted: b });
}

#[test]
fn an_unattested_step_between_attested_steps_is_named() {
    let repo = repo();
    activate(&repo.root);
    let a = record(&repo);
    let b = commit(&repo.root, "src/b.rs");
    attest(&repo.work, &a, &b);
    let c = commit(&repo.root, "src/c.rs");
    let d = commit(&repo.root, "src/d.rs");
    attest(&repo.work, &c, &d);

    let paths = vec!["src/c.rs".to_string()];
    let expected = HoldReason::Unattested {
        from: b,
        to: c,
        paths,
    };
    assert_eq!(reasons(guard(&repo)), vec![expected]);
}

#[test]
fn an_abort_line_cancels_its_attestation() {
    let repo = repo();
    activate(&repo.root);
    let a = record(&repo);
    let b = commit(&repo.root, "src/b.rs");
    attest(&repo.work, &a, &b);
    ledger_line(&repo.work, &format!("abort {a} {b} refs/heads/main"));

    match &reasons(guard(&repo))[..] {
        [HoldReason::Unattested { from, to, .. }] => assert_eq!((from, to), (&a, &b)),
        other => panic!("expected one Unattested reason, got {other:?}"),
    }
}

#[test]
fn a_merged_stage_branch_is_not_stage_work() {
    let repo = repo();
    stage_branch(&repo.root, &["a.txt"]);
    git(&repo.root, &["merge", "--no-ff", "-m", "merge", "loom/s"]);
    record(&repo);
    let b = commit(&repo.root, "src/op.rs");

    assert_eq!(guard(&repo), GuardState::Clear { accepted: b });
}

#[test]
fn an_orphan_stage_branch_is_neither_stage_work_nor_unevaluable() {
    let repo = repo();
    let orphan = crafted(&repo.root, &tip(&repo.root), "x.txt", false);
    git(&repo.root, &["update-ref", "refs/heads/loom/x", &orphan]);
    record(&repo);
    let b = commit(&repo.root, "src/op.rs");

    assert_eq!(guard(&repo), GuardState::Clear { accepted: b });
}

#[test]
fn a_stage_ref_to_a_blob_is_skipped() {
    let repo = repo();
    let blob = git(&repo.root, &["hash-object", "-w", "README.md"]);
    // git refuses to point a branch at a blob; a session can write the file.
    let loose = repo.root.join(".git/refs/heads/loom");
    std::fs::create_dir_all(&loose).unwrap();
    std::fs::write(loose.join("blob"), format!("{blob}\n")).unwrap();
    let listed = ["for-each-ref", "--format=%(objecttype)", "refs/heads/loom/"];
    assert_eq!(git(&repo.root, &listed), "blob");
    record(&repo);
    let b = commit(&repo.root, "src/op.rs");

    assert_eq!(guard(&repo), GuardState::Clear { accepted: b });
}

#[test]
fn malformed_and_dangling_ledger_lines_are_skipped() {
    let repo = repo();
    activate(&repo.root);
    let a = record(&repo);
    let b = commit(&repo.root, "src/b.rs");
    attest(&repo.work, &a, &b);
    assert_eq!(
        guard(&repo),
        GuardState::Clear {
            accepted: b.clone()
        }
    );
    let (zero, gone) = ("0".repeat(40), "e".repeat(40));
    attest(&repo.work, &b, &b);
    attest(&repo.work, &b, &zero);
    attest(&repo.work, &b, &gone);
    attest(&repo.work, &gone, &b);
    ledger_line(
        &repo.work,
        &format!("attest ref:refs/heads/x {b} refs/heads/main"),
    );
    ledger_line(&repo.work, "garbage");
    let c = commit(&repo.root, "doc/loom/knowledge/c.md");

    assert_eq!(guard(&repo), GuardState::Clear { accepted: c });
}

#[test]
fn a_ledger_of_resets_does_not_cycle() {
    let repo = repo();
    activate(&repo.root);
    let a = record(&repo);
    let e = commit(&repo.root, "src/e.rs");
    attest(&repo.work, &a, &e);
    let f = commit(&repo.root, "src/f.rs");
    attest(&repo.work, &e, &f);
    git(&repo.root, &["reset", "--hard", &e]);
    attest(&repo.work, &f, &e);
    let g = commit(&repo.root, "src/g.rs");
    attest(&repo.work, &e, &g);
    git(&repo.root, &["reset", "--hard", &f]);
    attest(&repo.work, &g, &f);
    let k = commit(&repo.root, "doc/loom/knowledge/k.md");

    assert_eq!(guard(&repo), GuardState::Clear { accepted: k });
}

#[test]
fn knowledge_gaps_between_attested_steps_are_walked_to_the_tip() {
    let repo = repo();
    activate(&repo.root);
    record(&repo);
    let k1 = commit(&repo.root, "doc/loom/knowledge/1.md");
    let b = commit(&repo.root, "src/b.rs");
    attest(&repo.work, &k1, &b);
    let k2 = commit(&repo.root, "doc/loom/knowledge/2.md");
    let c = commit(&repo.root, "src/c.rs");
    attest(&repo.work, &k2, &c);
    let k3 = commit(&repo.root, "doc/loom/knowledge/3.md");

    // Five moves over two ledger steps: more iterations than steps + 2.
    assert_eq!(guard(&repo), GuardState::Clear { accepted: k3 });
}

#[test]
fn a_backward_attested_step_is_followed() {
    let repo = repo();
    activate(&repo.root);
    let a0 = tip(&repo.root);
    let a = commit(&repo.root, "src/a.rs");
    record(&repo);
    git(&repo.root, &["reset", "--hard", &a0]);
    attest(&repo.work, &a, &a0);
    let b = commit(&repo.root, "src/b.rs");
    attest(&repo.work, &a0, &b);
    git(&repo.root, &["merge", "--no-ff", "-m", "merge", &a]);
    let m = tip(&repo.root);
    attest(&repo.work, &b, &m);

    assert_eq!(guard(&repo), GuardState::Clear { accepted: m });
}

#[test]
fn a_gap_after_a_backward_attested_step_starts_where_the_chain_ends() {
    let repo = repo();
    activate(&repo.root);
    let a0 = tip(&repo.root);
    let a = commit(&repo.root, "src/a.rs");
    record(&repo);
    git(&repo.root, &["reset", "--hard", &a0]);
    attest(&repo.work, &a, &a0);
    let b = commit(&repo.root, "src/b.rs");
    attest(&repo.work, &a0, &b);
    git(&repo.root, &["merge", "--no-ff", "-m", "merge", &a]);
    let m = tip(&repo.root);
    attest(&repo.work, &b, &m);
    let c = commit(&repo.root, "src/c.rs");

    let paths = vec!["src/c.rs".to_string()];
    let expected = HoldReason::Unattested {
        from: m,
        to: c,
        paths,
    };
    assert_eq!(reasons(guard(&repo)), vec![expected]);
}

/// Commit `src/a.rs` and accept it, then reset `main` to its parent and back
/// with both steps attested; returns the accepted commit.
fn reset_and_undo(repo: &Repo) -> String {
    let a0 = tip(&repo.root);
    let a = commit(&repo.root, "src/a.rs");
    record(repo);
    git(&repo.root, &["reset", "--hard", &a0]);
    attest(&repo.work, &a, &a0);
    git(&repo.root, &["reset", "--hard", &a]);
    attest(&repo.work, &a0, &a);
    a
}

#[test]
fn a_knowledge_commit_after_a_reset_and_its_undo_is_accepted() {
    let repo = repo();
    activate(&repo.root);
    reset_and_undo(&repo);
    let k = commit(&repo.root, "doc/loom/knowledge/x.md");

    assert_eq!(guard(&repo), GuardState::Clear { accepted: k });
}

#[test]
fn a_gap_after_a_reset_and_its_undo_starts_at_the_accepted_tip() {
    let repo = repo();
    activate(&repo.root);
    let a = reset_and_undo(&repo);
    let c = commit(&repo.root, "src/c.rs");

    let paths = vec!["src/c.rs".to_string()];
    let expected = HoldReason::Unattested {
        from: a,
        to: c,
        paths,
    };
    assert_eq!(reasons(guard(&repo)), vec![expected]);
}

#[test]
fn a_held_target_that_moves_again_is_evaluated_again() {
    let repo = repo();
    record(&repo);
    let b = commit(&repo.root, ".claude/settings.json");
    assert_eq!(held(guard(&repo)).observed, b);
    let since = recorded_hold(&repo.work, "main").unwrap().unwrap().since;
    assert_eq!(held(guard(&repo)).since, since);

    let c = commit(&repo.root, "src/c.rs");

    let hold = held(guard(&repo));
    assert_eq!(hold.observed, c);
    assert_eq!(recorded_hold(&repo.work, "main").unwrap(), Some(hold));
}

#[test]
fn a_full_ref_target_is_keyed_by_its_branch_name() {
    let repo = repo();
    let main = tip(&repo.root);

    let state = check(&repo.root, &repo.work, "refs/heads/main").unwrap();

    assert_eq!(
        state,
        Some(GuardState::Clear {
            accepted: main.clone()
        })
    );
    assert_eq!(accepted_tip(&repo.work, "main").unwrap(), Some(main));
    let refs = std::fs::read_to_string(repo.work.join(REFS_FILE)).unwrap();
    assert!(refs.contains("\nref refs/heads/main\n"), "{refs}");
    let json = std::fs::read_to_string(repo.work.join(RECORD_FILE)).unwrap();
    assert!(
        json.contains("\"main\"") && !json.contains("refs/heads"),
        "{json}"
    );
}
