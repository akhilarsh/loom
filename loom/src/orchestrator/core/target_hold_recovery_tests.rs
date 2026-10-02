//! Recovery's merged-flag check against the target guard: a definite "not in
//! the accepted tip" reverts `merged` whatever the live target holds, a
//! guard record that cannot be read leaves it alone.

use std::path::Path;
use std::process::Command;

use serial_test::serial;
use tempfile::TempDir;

use crate::fs::permissions::scratch_home::ScratchHome;
use crate::fs::work_dir::write_terminal_config;
use crate::git::target_guard::{HoldReason, RECORD_FILE};
use crate::models::session::{SessionBackendKind, TerminalConfig};
use crate::models::stage::{Stage, StageStatus};
use crate::orchestrator::core::recovery::Recovery;
use crate::orchestrator::core::{Orchestrator, OrchestratorConfig};
use crate::orchestrator::scheduling_report::BlockReason;
use crate::plan::{schema::StageDefinition, ExecutionGraph};
use crate::verify::transitions::{load_stage, save_stage};

const ID: &str = "s";

/// Run `git` in `dir` with ambient configuration shut out, assert it
/// succeeded, and return its trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", dir.join(".loom-test-no-global"))
        .env("GIT_CONFIG_SYSTEM", dir.join(".loom-test-no-system"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t.com")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "git {args:?} failed: {stderr}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Commit `name` holding `text` on the checked-out branch; returns the tip.
fn commit(dir: &Path, name: &str, text: &str) -> String {
    std::fs::write(dir.join(name), text).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", name]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A repository on `main` holding `seed.txt`, with the state directories
/// excluded from its status as in a real project.
fn repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    let root = repo.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.email", "t@t.com"]);
    git(root, &["config", "user.name", "t"]);
    std::fs::write(root.join(".git/info/exclude"), ".loom/\n.worktrees/\n").unwrap();
    commit(root, "seed.txt", "seed");
    repo
}

/// `loom/s` with one commit beyond `main`, and `main` checked out; returns
/// the branch tip.
fn side_commit(root: &Path) -> String {
    git(root, &["checkout", "-q", "-b", "loom/s"]);
    let work = commit(root, "work.txt", "stage work");
    git(root, &["checkout", "-q", "main"]);
    work
}

/// An orchestrator over `root` on the tmux lane, so building it never probes
/// the host for a terminal emulator.
fn orchestrator(root: &Path) -> Orchestrator {
    let work_dir = root.join(".loom").join("work");
    let backend = SessionBackendKind::Tmux;
    write_terminal_config(&work_dir, &TerminalConfig { backend }).unwrap();
    let config = OrchestratorConfig {
        work_dir,
        repo_root: root.to_path_buf(),
        base_branch: Some("main".to_string()),
        enable_skill_routing: false,
        ..Default::default()
    };
    let definition = StageDefinition {
        id: ID.to_string(),
        name: ID.to_string(),
        working_dir: ".".into(),
        ..Default::default()
    };
    Orchestrator::new(config, ExecutionGraph::build(vec![definition]).unwrap()).unwrap()
}

/// Save a completed stage flagged `merged` at `commit`.
fn save_merged(orchestrator: &Orchestrator, commit: &str) {
    let stage = Stage {
        id: ID.to_string(),
        status: StageStatus::Completed,
        merged: true,
        completed_commit: Some(commit.to_string()),
        ..Stage::default()
    };
    save_stage(&stage, &orchestrator.config.work_dir).unwrap();
}

#[test]
#[serial]
fn an_unreadable_guard_record_keeps_a_merged_stage_merged() {
    let repo = repo();
    let root = repo.path();
    let mut orchestrator = orchestrator(root);
    save_merged(&orchestrator, &git(root, &["rev-parse", "main"]));
    let record = orchestrator.config.work_dir.join(RECORD_FILE);
    std::fs::write(record, "{ not json").unwrap();

    orchestrator.sync_graph_with_stage_files().unwrap();

    let stage = load_stage(ID, &orchestrator.config.work_dir).unwrap();
    assert!(stage.merged, "an unreadable guard record reverted merged");
}

#[test]
#[serial]
fn a_commit_outside_the_accepted_tip_still_reverts_merged() {
    let repo = repo();
    let root = repo.path();
    let side = side_commit(root);
    let mut orchestrator = orchestrator(root);
    assert_eq!(orchestrator.check_target_guard(), None);
    assert!(orchestrator.config.work_dir.join(RECORD_FILE).exists());
    save_merged(&orchestrator, &side);

    orchestrator.sync_graph_with_stage_files().unwrap();

    let stage = load_stage(ID, &orchestrator.config.work_dir).unwrap();
    assert!(
        !stage.merged,
        "a commit outside the accepted tip stayed merged"
    );
}

/// `main` moved onto `commit` the way an agent would, with hooks off.
fn agent_move(root: &Path, commit: &str) {
    let args = ["-c", "core.hooksPath=/dev/null", "update-ref"];
    git(root, &[&args[..], &["refs/heads/main", commit]].concat());
}

#[test]
#[serial]
fn a_merged_commit_only_the_held_tip_contains_reverts_merged() {
    let repo = repo();
    let root = repo.path();
    let side = side_commit(root);
    let mut orchestrator = orchestrator(root);
    assert_eq!(orchestrator.check_target_guard(), None);
    agent_move(root, &side);
    let hold = orchestrator
        .check_target_guard()
        .expect("the move was not held");
    assert_eq!(hold.observed, side);
    save_merged(&orchestrator, &side);

    orchestrator.sync_graph_with_stage_files().unwrap();

    let stage = load_stage(ID, &orchestrator.config.work_dir).unwrap();
    assert!(
        !stage.merged,
        "a commit only the held live tip contains stayed merged"
    );
}

#[test]
#[serial]
fn a_rewrite_dropping_a_merged_commit_keeps_it_merged() {
    let repo = repo();
    let root = repo.path();
    let seed = git(root, &["rev-parse", "main"]);
    let merged = commit(root, "merged.txt", "merged work");
    let mut orchestrator = orchestrator(root);
    assert_eq!(orchestrator.check_target_guard(), None);
    git(root, &["reset", "-q", "--hard", &seed]);
    let rewritten = commit(root, "rewrite.txt", "rewritten history");
    let hold = orchestrator
        .check_target_guard()
        .expect("the rewrite was not held");
    assert_eq!(hold.observed, rewritten);
    assert!(
        hold.reasons.contains(&HoldReason::NotFastForward),
        "{hold:?}"
    );
    save_merged(&orchestrator, &merged);

    orchestrator.sync_graph_with_stage_files().unwrap();

    let stage = load_stage(ID, &orchestrator.config.work_dir).unwrap();
    assert!(
        stage.merged,
        "a rewrite of the live target reverted a merge the accepted tip holds"
    );
}

#[test]
#[serial]
fn an_unreadable_guard_record_blocks_a_new_stage_spawn() {
    let repo = repo();
    let root = repo.path();
    let mut orchestrator = orchestrator(root);
    let record = orchestrator.config.work_dir.join(RECORD_FILE);
    std::fs::write(record, "{ not json").unwrap();
    let stage = Stage {
        id: ID.to_string(),
        status: StageStatus::Queued,
        ..Stage::default()
    };
    let _home = ScratchHome::set();

    let resolved = orchestrator.resolve_worktree(ID, &stage).unwrap();

    assert!(resolved.is_none(), "a worktree was created");
    assert_eq!(
        orchestrator.spawn_blocks.get(ID),
        Some(&BlockReason::TargetHeld),
        "the spawn was not held for the target"
    );
    let branch = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", "refs/heads/loom/s"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!branch.status.success(), "loom/s was created");
}

#[test]
#[serial]
fn a_repeating_probe_error_is_kept_once_until_a_probe_answers() {
    let repo = repo();
    let root = repo.path();
    let mut orchestrator = orchestrator(root);
    let main = git(root, &["rev-parse", "main"]);
    let record = orchestrator.config.work_dir.join(RECORD_FILE);
    std::fs::write(&record, "{ not json").unwrap();

    assert!(orchestrator.probe_accepted(ID, &main, "main").is_err());
    let logged = orchestrator.merge_probe_error.clone();
    assert!(logged.is_some(), "the probe error was not logged");
    assert!(orchestrator.probe_accepted("t", &main, "main").is_err());
    assert_eq!(orchestrator.merge_probe_error, logged);

    std::fs::remove_file(&record).unwrap();
    assert!(orchestrator.probe_accepted(ID, &main, "main").unwrap());
    assert_eq!(orchestrator.merge_probe_error, None);
}
