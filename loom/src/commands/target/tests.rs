//! `loom target` report, accept and refusal logic.

use super::accept::{checkout_note, refuse_inside_session};
use super::refuse_unreviewed_move;
use super::status::report;
use crate::git::target_guard::test_support::{
    activate, commit_file, git, held_repo, recorded, repo, Repo,
};
use crate::git::target_guard::{check, Accepted, RECORD_FILE};
use serial_test::serial;

pub(super) fn report_of(repo: &Repo) -> String {
    report(&repo.root, &repo.work, "main").unwrap()
}

/// `text` without its `Attestation:` line, which names a machine-dependent
/// reason; the line itself must start with `expected`.
fn without_attestation(text: &str, expected: &str) -> Vec<String> {
    let (attestation, rest): (Vec<&str>, Vec<&str>) = text
        .lines()
        .partition(|line| line.starts_with("Attestation:"));
    assert_eq!(attestation.len(), 1, "{text}");
    assert!(attestation[0].starts_with(expected), "{text}");
    rest.into_iter().map(str::to_string).collect()
}

#[test]
fn report_without_a_record_says_the_target_is_not_guarded_yet() {
    let repo = repo();
    let tip = git(&repo.root, &["rev-parse", "main"]);

    let lines = without_attestation(&report_of(&repo), "Attestation: off (");

    assert_eq!(
        lines,
        [
            "Target: main".to_string(),
            "Accepted: not recorded yet (the daemon records it at start)".to_string(),
            format!("Current: {tip}"),
            "State: not guarded yet".to_string(),
        ]
    );
}

#[test]
fn report_with_an_unreadable_record_still_reports_and_offers_accept() {
    let (repo, _accepted) = recorded();
    let tip = git(&repo.root, &["rev-parse", "main"]);
    std::fs::write(repo.work.join(RECORD_FILE), "{ not json").unwrap();
    let text = report_of(&repo);
    assert!(text.contains("Target: main"), "{text}");
    assert!(text.contains(&format!("Current: {tip}")), "{text}");
    assert!(text.contains(RECORD_FILE), "{text}");
    assert!(text.contains("unreadable"), "{text}");
    assert!(text.contains("unevaluable"), "{text}");
    assert!(
        text.contains(&format!("loom target accept --to {tip}")),
        "{text}"
    );
    assert!(!text.contains("Restore"), "{text}");
}

#[test]
fn report_with_a_clear_record_is_in_sync() {
    let (repo, accepted) = recorded();

    let lines = without_attestation(&report_of(&repo), "Attestation: off (");

    assert_eq!(
        lines,
        [
            "Target: main".to_string(),
            format!("Accepted: {accepted}"),
            format!("Current: {accepted}"),
            "State: in sync".to_string(),
        ]
    );
}

#[test]
fn report_with_a_hold_lists_review_accept_and_both_restore_commands() {
    let (repo, accepted, moved) = held_repo(true);

    let text = report_of(&repo);

    let lines = without_attestation(&text, "Attestation: off (");
    assert!(lines[3].starts_with("State: HELD since "), "{text}");
    assert_eq!(
        lines[4..],
        [
            "  - touches .claude/settings.json".to_string(),
            format!("Review: git log --oneline {accepted}..{moved}"),
            format!("        git diff --stat {accepted} {moved}"),
            format!("Accept: loom target accept --to {moved}"),
            "Restore:".to_string(),
            format!("  git update-ref refs/heads/main {accepted} {moved}"),
            format!(
                "  git read-tree -m -u {moved} {accepted}  # run in {}",
                repo.root.display()
            ),
        ]
    );
}

#[test]
fn report_gives_one_restore_command_when_the_target_is_not_checked_out() {
    let (repo, accepted, moved) = held_repo(true);
    git(&repo.root, &["checkout", "-q", "--detach"]);

    let text = report_of(&repo);

    assert!(text.contains(&format!(
        "  git update-ref refs/heads/main {accepted} {moved}"
    )));
    assert!(!text.contains("git read-tree"), "{text}");
}

#[test]
fn report_of_a_move_not_yet_evaluated_says_what_the_next_check_does() {
    let (repo, accepted) = recorded();
    let plain = commit_file(&repo.root, "notes.txt", "notes\n");

    let lines = without_attestation(&report_of(&repo), "Attestation: off (");

    assert_eq!(
        lines[3..],
        [
            format!(
                "State: moved {}..{}, not yet evaluated",
                &accepted[..12],
                &plain[..12]
            ),
            "  it would be accepted".to_string(),
        ]
    );
    commit_file(&repo.root, ".claude/settings.json", "{}\n");
    let lines = without_attestation(&report_of(&repo), "Attestation: off (");
    assert_eq!(
        lines[4..],
        ["  it would be held:", "  - touches .claude/settings.json"]
    );
}

#[test]
fn report_says_a_latched_record_holds_unattested_moves_while_attestation_is_off() {
    let repo = repo();
    activate(&repo.root);
    check(&repo.root, &repo.work, "main").unwrap();
    std::fs::remove_file(repo.root.join(".git/hooks/reference-transaction")).unwrap();

    let text = report_of(&repo);

    let line = text
        .lines()
        .find(|l| l.starts_with("Attestation:"))
        .unwrap();
    assert_eq!(
        line,
        "Attestation: off (loom's reference-transaction hook is not installed); this run \
         recorded it on, so every move without a ledger line holds until loom target accept"
    );
}

#[test]
fn report_says_attestation_is_on_when_the_hook_is_active() {
    let repo = repo();
    activate(&repo.root);

    assert!(report_of(&repo).contains("\nAttestation: on\n"));
}

/// Pins `LOOM_SESSION_ID` (`None` removes it) for one test and puts back the
/// original value on drop, a panic included: the stage running this suite has
/// the variable set.
struct SessionEnv(Option<std::ffi::OsString>);

impl SessionEnv {
    fn set(value: Option<&str>) -> Self {
        let original = std::env::var_os("LOOM_SESSION_ID");
        match value {
            Some(value) => std::env::set_var("LOOM_SESSION_ID", value),
            None => std::env::remove_var("LOOM_SESSION_ID"),
        }
        Self(original)
    }
}

impl Drop for SessionEnv {
    fn drop(&mut self) {
        match &self.0 {
            Some(original) => std::env::set_var("LOOM_SESSION_ID", original),
            None => std::env::remove_var("LOOM_SESSION_ID"),
        }
    }
}

#[test]
#[serial]
fn accept_refuses_inside_a_loom_session() {
    let _session = SessionEnv::set(Some("session-7"));

    let message = refuse_inside_session().unwrap_err().to_string();

    assert!(
        message.contains("this is loom session session-7"),
        "{message}"
    );
    assert!(
        message.contains("a stage agent cannot accept it"),
        "{message}"
    );
}

#[test]
#[serial]
fn accept_proceeds_outside_a_loom_session() {
    let _session = SessionEnv::set(None);
    assert!(refuse_inside_session().is_ok());
    std::env::set_var("LOOM_SESSION_ID", "");
    assert!(refuse_inside_session().is_ok());
}

#[test]
fn checkout_note_names_the_read_tree_command_when_the_index_is_behind() {
    let (repo, accepted) = recorded();
    let moved = commit_file(&repo.root, "notes.txt", "notes\n");
    // The ref moved to `moved` while the checkout's index stayed at `accepted`.
    git(&repo.root, &["reset", "-q", &accepted]);
    git(&repo.root, &["update-ref", "refs/heads/main", &moved]);
    let step = Accepted {
        from: accepted.clone(),
        to: moved.clone(),
    };

    let note = checkout_note(&repo.root, "main", &step).unwrap().unwrap();

    assert_eq!(
        note,
        format!(
            "Your checkout of main still holds files from before the move. Bring it along \
             (keeps your local edits): git read-tree -m -u {accepted} {moved}"
        )
    );
}

#[test]
fn checkout_note_is_silent_when_the_index_is_current_or_the_target_is_not_checked_out() {
    let (repo, accepted) = recorded();
    let moved = commit_file(&repo.root, "notes.txt", "notes\n");
    let step = Accepted {
        from: accepted,
        to: moved,
    };
    assert_eq!(checkout_note(&repo.root, "main", &step).unwrap(), None);

    git(&repo.root, &["checkout", "-q", "--detach"]);
    git(&repo.root, &["update-ref", "refs/heads/main", &step.from]);
    assert_eq!(checkout_note(&repo.root, "main", &step).unwrap(), None);
}

#[test]
fn refusal_names_a_recorded_hold() {
    let (repo, _accepted, _moved) = held_repo(true);

    let message = refuse_unreviewed_move(&repo.root).unwrap_err().to_string();

    assert!(message.contains("the target branch main has a move loom did not accept"));
    assert!(
        message.contains("touches .claude/settings.json"),
        "{message}"
    );
    assert!(message.contains("loom target status"), "{message}");
}

#[test]
fn refusal_names_a_hold_the_next_check_would_record() {
    let (repo, _accepted, _moved) = held_repo(false);

    let error = refuse_unreviewed_move(&repo.root).unwrap_err();

    assert!(error.to_string().contains("has a move loom did not accept"));
    assert_eq!(
        crate::git::target_guard::recorded_hold(&repo.work, "main").unwrap(),
        None,
        "the refusal must not write the record"
    );
}

#[test]
fn refusal_names_an_unreadable_record() {
    let (repo, _accepted) = recorded();
    std::fs::write(repo.work.join(RECORD_FILE), "{ not json").unwrap();

    let message = format!("{:#}", refuse_unreviewed_move(&repo.root).unwrap_err());

    assert!(message.contains(RECORD_FILE), "{message}");
}

#[test]
fn refusal_names_the_record_to_remove_when_the_target_branch_is_gone() {
    let (repo, _accepted) = recorded();
    git(&repo.root, &["checkout", "-q", "--detach"]);
    git(&repo.root, &["branch", "-q", "-D", "main"]);

    let message = refuse_unreviewed_move(&repo.root).unwrap_err().to_string();

    assert!(
        message.contains("the target branch main no longer exists"),
        "{message}"
    );
    let record = repo.work.join(RECORD_FILE);
    assert!(message.contains(&record.display().to_string()), "{message}");
}

#[test]
fn no_refusal_for_a_clear_or_missing_record_or_a_malformed_config() {
    let repo = repo();
    refuse_unreviewed_move(&repo.root).unwrap();

    check(&repo.root, &repo.work, "main").unwrap();
    commit_file(&repo.root, "notes.txt", "notes\n");
    std::fs::write(repo.work.join("config.toml"), "= not toml").unwrap();

    refuse_unreviewed_move(&repo.root).unwrap();
}
