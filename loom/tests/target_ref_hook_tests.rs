//! Loom's `reference-transaction` hook under real git: which moves of the
//! guarded target it attests, which it refuses, and the `core.hooksPath`
//! settings that make git skip it (and turn attestation off).
//!
//! Every git command here runs with the user's and system's config shut out
//! and sets or removes `LOOM_SESSION_ID` explicitly.

use loom::git::configured_hooks_path;
use loom::git::hooks::{install_reference_transaction_hook, HookInstall};
use loom::git::target_guard::{
    attestation_mode, check, AttestationMode, GuardState, LEDGER_FILE, REFS_FILE,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

/// git in `dir`; `session` sets `LOOM_SESSION_ID`, `None` removes it.
fn command(dir: &Path, args: &[&str], session: Option<&str>) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", dir.join(".loom-test-no-global"))
        .env("GIT_CONFIG_SYSTEM", dir.join(".loom-test-no-system"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t.com");
    match session {
        Some(id) => cmd.env("LOOM_SESSION_ID", id),
        None => cmd.env_remove("LOOM_SESSION_ID"),
    };
    cmd
}

fn run(dir: &Path, args: &[&str], session: Option<&str>) -> Output {
    command(dir, args, session).output().unwrap()
}

/// git in `dir` outside any session, with `input` on its stdin.
fn run_with_input(dir: &Path, args: &[&str], input: &str) -> Output {
    let mut cmd = command(dir, args, None);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input.as_bytes()).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = run(dir, args, None);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        stderr(&out)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Commit `path` in the checkout at `dir` outside any session.
fn commit(dir: &Path, path: &str) -> String {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, path).unwrap();
    git(dir, &["add", path]);
    git(dir, &["commit", "-m", path]);
    git(dir, &["rev-parse", "HEAD"])
}

fn tip(root: &Path) -> String {
    git(root, &["rev-parse", "refs/heads/main"])
}

fn ledger(work: &Path) -> String {
    std::fs::read_to_string(work.join(LEDGER_FILE)).unwrap()
}

/// Replace the ledger with a directory, so no one can append to it.
fn make_ledger_unwritable(work: &Path) {
    let ledger = work.join(LEDGER_FILE);
    std::fs::remove_file(&ledger).unwrap();
    std::fs::create_dir_all(&ledger).unwrap();
}

fn ledger_dir_is_empty(work: &Path) -> bool {
    std::fs::read_dir(work.join(LEDGER_FILE))
        .unwrap()
        .next()
        .is_none()
}

struct Repo {
    _dir: TempDir,
    root: PathBuf,
    work: PathBuf,
    /// The linked worktree `.worktrees/s` on `loom/s`.
    wt: PathBuf,
    /// `main`'s tip when the guard recorded it.
    main: String,
    /// `loom/s`'s tip: a child of `main` adding `a.txt`.
    stage_tip: String,
}

/// A repository on `main` with one commit, a linked worktree `.worktrees/s`
/// on `loom/s` with one more, `.loom/work` created, the hook installed with
/// attestation on, and `main` recorded by `check` (which writes the refs
/// file and the empty ledger).
fn repo() -> Repo {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-b", "main"]);
    std::fs::write(root.join(".git/info/exclude"), ".loom/\n.worktrees/\n").unwrap();
    commit(&root, "README.md");
    let wt = root.join(".worktrees/s");
    git(
        &root,
        &["worktree", "add", "-b", "loom/s", wt.to_str().unwrap()],
    );
    let stage_tip = commit(&wt, "a.txt");
    let work = root.join(".loom/work");
    std::fs::create_dir_all(&work).unwrap();
    let installed = install_reference_transaction_hook(&root).unwrap();
    assert_eq!(installed, HookInstall::Installed);
    assert_eq!(
        attestation_mode(&root, &work),
        AttestationMode::Active,
        "attestation must be on (is core.hooksPath set at global or system scope?)"
    );
    let main = tip(&root);
    let state = check(&root, &work, "main")
        .unwrap()
        .expect("no other merge lock holder");
    assert_eq!(
        state,
        GuardState::Clear {
            accepted: main.clone()
        }
    );
    assert_eq!(ledger(&work), "");
    Repo {
        _dir: dir,
        root,
        work,
        wt,
        main,
        stage_tip,
    }
}

/// `git update-ref refs/heads/main <stage tip>` in `dir` as `session`.
fn move_main_to_stage_tip(repo: &Repo, dir: &Path, session: Option<&str>) -> Output {
    let args = ["update-ref", "refs/heads/main", repo.stage_tip.as_str()];
    run(dir, &args, session)
}

#[test]
fn stage_branch_commit_writes_no_ledger_line() {
    let repo = repo();
    commit(&repo.wt, "b.txt");
    assert_eq!(ledger(&repo.work), "");
}

#[test]
fn aborted_transaction_leaves_an_attest_and_a_matching_abort() {
    let repo = repo();
    let (main, to) = (&repo.main, &repo.stage_tip);
    let input = format!("start\nupdate refs/heads/main {to}\nprepare\nabort\n");
    let out = run_with_input(&repo.root, &["update-ref", "--stdin"], &input);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(tip(&repo.root), repo.main);
    let attest = format!("attest {main} {to} refs/heads/main\n");
    let abort = format!("abort {main} {to} refs/heads/main\n");
    assert_eq!(ledger(&repo.work), attest + &abort);
}

#[test]
fn without_a_refs_file_the_hook_does_nothing() {
    let repo = repo();
    std::fs::remove_file(repo.work.join(REFS_FILE)).unwrap();
    make_ledger_unwritable(&repo.work);
    let out = move_main_to_stage_tip(&repo, &repo.wt, Some("s1"));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "");
    assert_eq!(tip(&repo.root), repo.stage_tip);
    assert!(ledger_dir_is_empty(&repo.work));
}

#[test]
fn worktree_move_is_attested_in_the_main_checkout_ledger() {
    let repo = repo();
    let out = move_main_to_stage_tip(&repo, &repo.wt, None);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "", "the hook must be silent on success");
    assert_eq!(tip(&repo.root), repo.stage_tip);
    let expected = format!("attest {} {} refs/heads/main\n", repo.main, repo.stage_tip);
    assert_eq!(ledger(&repo.work), expected);
}

#[test]
fn unwritable_ledger_outside_a_session_allows_the_move() {
    let repo = repo();
    make_ledger_unwritable(&repo.work);
    let out = move_main_to_stage_tip(&repo, &repo.wt, None);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(tip(&repo.root), repo.stage_tip);
    assert!(ledger_dir_is_empty(&repo.work));
}

#[test]
fn session_knowledge_commit_with_a_quoted_path_is_refused() {
    let repo = repo();
    make_ledger_unwritable(&repo.work);
    let path = "doc/loom/knowledge/é.md";
    let file = repo.root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, path).unwrap();
    git(&repo.root, &["add", path]);
    let out = run(&repo.root, &["commit", "-m", path], Some("k1"));
    assert!(
        !out.status.success(),
        "the hook let a session commit {path}"
    );
    let refusal = "loom: refusing to move refs/heads/main from loom session k1";
    assert!(stderr(&out).contains(refusal), "{}", stderr(&out));
    assert_eq!(tip(&repo.root), repo.main);
}

#[test]
fn session_push_from_the_worktree_is_refused() {
    let repo = repo();
    // With `main` checked out, git itself refuses the push.
    git(&repo.root, &["checkout", "--detach"]);
    make_ledger_unwritable(&repo.work);
    let out = run(&repo.wt, &["push", ".", "HEAD:main"], Some("s1"));
    assert!(!out.status.success(), "the hook let a session push main");
    let refusal = "loom: refusing to move refs/heads/main from loom session s1";
    assert!(stderr(&out).contains(refusal), "{}", stderr(&out));
    assert_eq!(tip(&repo.root), repo.main);
}

#[test]
fn hooks_path_dev_null_moves_the_target_without_a_line() {
    let repo = repo();
    let to = repo.stage_tip.as_str();
    let args = [
        "-c",
        "core.hooksPath=/dev/null",
        "update-ref",
        "refs/heads/main",
        to,
    ];
    git(&repo.root, &args);
    assert_eq!(tip(&repo.root), repo.stage_tip);
    assert_eq!(ledger(&repo.work), "");
}

/// `core.hooksPath` is `/dev/null` for git and for loom's reads, attestation
/// is off, and a host commit on `main` writes no ledger line.
fn assert_hooks_skipped(repo: &Repo) {
    let configured = configured_hooks_path(&repo.root);
    assert_eq!(configured.as_deref(), Some("/dev/null"));
    let mode = attestation_mode(&repo.root, &repo.work);
    assert!(matches!(mode, AttestationMode::Off { .. }), "{mode:?}");
    commit(&repo.root, "src/y.rs");
    assert_eq!(ledger(&repo.work), "");
}

#[test]
fn included_hooks_path_turns_attestation_off() {
    let repo = repo();
    let include = repo.root.join(".git/loom-test-include");
    std::fs::write(&include, "[core]\n\thooksPath = /dev/null\n").unwrap();
    git(
        &repo.root,
        &["config", "include.path", include.to_str().unwrap()],
    );
    assert_hooks_skipped(&repo);
}

#[test]
fn worktree_config_hooks_path_turns_attestation_off() {
    let repo = repo();
    git(&repo.root, &["config", "extensions.worktreeConfig", "true"]);
    git(
        &repo.root,
        &["config", "--worktree", "core.hooksPath", "/dev/null"],
    );
    assert_hooks_skipped(&repo);
}
