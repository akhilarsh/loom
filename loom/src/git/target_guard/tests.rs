//! The record, refs file, ledger, accept, lock and restore commands, the
//! runner's replace-ref and graft shutoff, and the fixtures the guard's other
//! test files share.

use super::*;
use crate::git::merge::control_paths::changed_paths;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// A scratch repository on `main` and its state directory `root/.loom/work`.
pub(super) struct Repo {
    _dir: TempDir,
    pub(super) root: PathBuf,
    pub(super) work: PathBuf,
}

/// Trimmed stdout of a git command that must succeed, with ambient config
/// shut out and `envs` added.
fn git_env(dir: &Path, args: &[&str], envs: &[(&str, &Path)]) -> String {
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
        .envs(envs.iter().copied())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    git_env(dir, args, &[])
}

/// A repository on `main` with `README.md` committed, `.loom/` and
/// `.worktrees/` excluded, and the state directory created.
pub(super) fn repo() -> Repo {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t.com"]);
    std::fs::write(root.join(".git/info/exclude"), ".loom/\n.worktrees/\n").unwrap();
    commit(&root, "README.md");
    let work = root.join(".loom/work");
    std::fs::create_dir_all(&work).unwrap();
    Repo {
        _dir: dir,
        root,
        work,
    }
}

/// Commit `path` (its content is its name) on the branch checked out at
/// `dir`; returns the commit.
pub(super) fn commit(dir: &Path, path: &str) -> String {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, path).unwrap();
    git(dir, &["add", path]);
    git(dir, &["commit", "-m", path]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A commit on no branch whose tree is `base`'s plus `path`, a child of
/// `base` or (`parent` unset) a root commit.
pub(super) fn crafted(root: &Path, base: &str, path: &str, parent: bool) -> String {
    let index = root.join(".loom/crafted-index");
    let _ = std::fs::remove_file(&index);
    let index_env = [("GIT_INDEX_FILE", index.as_path())];
    let blob_file = root.join(".loom/crafted-blob");
    std::fs::write(&blob_file, path).unwrap();
    let blob = git(root, &["hash-object", "-w", blob_file.to_str().unwrap()]);
    git_env(root, &["read-tree", base], &index_env);
    let entry = format!("100644,{blob},{path}");
    let add = ["update-index", "--add", "--cacheinfo", &entry];
    git_env(root, &add, &index_env);
    let tree = git_env(root, &["write-tree"], &index_env);
    let mut args = vec!["commit-tree", tree.as_str(), "-m", "crafted"];
    if parent {
        args.extend(["-p", base]);
    }
    git(root, &args)
}

pub(super) fn tip(root: &Path) -> String {
    git(root, &["rev-parse", "refs/heads/main"])
}

/// Point `main` at `commit` without touching the checkout.
pub(super) fn move_main(root: &Path, commit: &str) {
    git(root, &["update-ref", "refs/heads/main", commit]);
}

/// Install a stand-in for loom's hook, so `attestation_mode` is `Active`. It
/// is not executable, so git never runs it: the tests write the ledger.
pub(super) fn activate(root: &Path) {
    let hook = root.join(".git/hooks/reference-transaction");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\n# {HOOK_MARKER}\nexit 0\n")).unwrap();
    assert_eq!(
        attestation_mode(root, &root.join(".loom/work")),
        AttestationMode::Active,
        "attestation must be on (is core.hooksPath set at global or system scope?)"
    );
}

/// The ledger line the hook writes for a host move of `main`.
pub(super) fn attest(work: &Path, from: &str, to: &str) {
    append_attestation(work, "refs/heads/main", from, to).unwrap();
}

/// Append a raw line to the ledger.
pub(super) fn ledger_line(work: &Path, line: &str) {
    use std::io::Write;
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(work.join(LEDGER_FILE))
        .unwrap();
    writeln!(ledger, "{line}").unwrap();
}

/// `check` on `main`, which no other owner holds the merge lock for.
pub(super) fn guard(repo: &Repo) -> GuardState {
    check(&repo.root, &repo.work, "main").unwrap().unwrap()
}

/// Record the current tip; returns it.
pub(super) fn record(repo: &Repo) -> String {
    let main = tip(&repo.root);
    assert_eq!(
        guard(repo),
        GuardState::Clear {
            accepted: main.clone()
        }
    );
    main
}

pub(super) fn held(state: GuardState) -> Hold {
    match state {
        GuardState::Held(hold) => hold,
        other => panic!("expected a hold, got {other:?}"),
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn first_check_records_the_tip_the_refs_file_and_an_empty_ledger() {
    let repo = repo();
    let main = record(&repo);

    assert_eq!(accepted_tip(&repo.work, "main").unwrap(), Some(main));
    assert_eq!(
        read(&repo.work.join(REFS_FILE)),
        "# written by loom; read by .git/hooks/reference-transaction\n\
         ref refs/heads/main\n\
         allow doc/loom/knowledge/\n"
    );
    assert_eq!(read(&repo.work.join(LEDGER_FILE)), "");
    assert_eq!(
        guarded_refs(&repo.work).unwrap(),
        vec!["refs/heads/main".to_string()]
    );
    assert!(!attestation_latched(&repo.work, "main").unwrap());
}

#[test]
fn record_advance_from_a_stale_tip_changes_nothing() {
    let repo = repo();
    let a = record(&repo);
    let b = commit(&repo.root, "src/b.rs");
    let before = read(&repo.work.join(RECORD_FILE));

    record_advance(&repo.work, "main", &b, &a).unwrap();

    assert_eq!(read(&repo.work.join(RECORD_FILE)), before);
    record_advance(&repo.work, "refs/heads/main", &a, &b).unwrap();
    assert_eq!(accepted_tip(&repo.work, "main").unwrap(), Some(b));
}

#[test]
fn accept_takes_an_abbreviated_current_tip_and_refuses_another() {
    let repo = repo();
    let a = record(&repo);
    let b = commit(&repo.root, ".claude/settings.json");
    assert!(matches!(guard(&repo), GuardState::Held(_)));

    let error = accept(&repo.root, &repo.work, "main", &a).unwrap_err();
    assert!(error.to_string().contains("moved since you reviewed it"));

    let accepted = accept(&repo.root, &repo.work, "main", &b[..9]).unwrap();
    assert_eq!(
        accepted,
        Accepted {
            from: a,
            to: b.clone()
        }
    );
    assert_eq!(recorded_hold(&repo.work, "main").unwrap(), None);
    assert_eq!(guard(&repo), GuardState::Clear { accepted: b });
}

#[test]
fn a_corrupt_record_is_unevaluable_and_left_unchanged() {
    let repo = repo();
    std::fs::write(repo.work.join(RECORD_FILE), "{not json").unwrap();

    let hold = held(guard(&repo));

    assert_eq!(hold.accepted, "");
    assert_eq!(hold.observed, tip(&repo.root));
    assert!(matches!(
        &hold.reasons[..],
        [HoldReason::Unevaluable { .. }]
    ));
    assert_eq!(read(&repo.work.join(RECORD_FILE)), "{not json");
    assert!(restore_commands(&repo.root, "main", &hold)
        .unwrap()
        .is_empty());
    let b = tip(&repo.root);
    accept(&repo.root, &repo.work, "main", &b).unwrap();
    assert_eq!(accepted_tip(&repo.work, "main").unwrap(), Some(b));
}

#[test]
fn check_returns_none_while_another_owner_holds_the_merge_lock() {
    let repo = repo();
    let _owner = MergeLock::try_acquire_in(&repo.work).unwrap().unwrap();

    assert_eq!(check(&repo.root, &repo.work, "main").unwrap(), None);
    assert!(!repo.work.join(RECORD_FILE).exists());
}

#[test]
fn pending_hold_writes_nothing() {
    let repo = repo();
    assert_eq!(pending_hold(&repo.root, &repo.work, "main").unwrap(), None);
    assert!(!repo.work.join(RECORD_FILE).exists());
    record(&repo);
    commit(&repo.root, ".claude/settings.json");
    let before = read(&repo.work.join(RECORD_FILE));

    let hold = pending_hold(&repo.root, &repo.work, "main")
        .unwrap()
        .unwrap();

    assert!(matches!(
        &hold.reasons[..],
        [HoldReason::ControlPaths { .. }]
    ));
    assert_eq!(read(&repo.work.join(RECORD_FILE)), before);
    assert_eq!(recorded_hold(&repo.work, "main").unwrap(), None);
}

#[test]
fn restore_commands_add_a_read_tree_only_where_the_target_is_checked_out() {
    let repo = repo();
    let a = record(&repo);
    let b = commit(&repo.root, ".claude/settings.json");
    let hold = held(guard(&repo));

    let commands = restore_commands(&repo.root, "main", &hold).unwrap();
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert_eq!(
        commands[0],
        format!("git update-ref refs/heads/main {a} {b}")
    );
    assert!(commands[1].starts_with(&format!("git read-tree -m -u {b} {a}")));

    git(&repo.root, &["checkout", "--detach"]);
    let commands = restore_commands(&repo.root, "main", &hold).unwrap();
    assert_eq!(commands.len(), 1, "{commands:?}");
    assert_eq!(
        accept_command("main", &hold),
        format!("loom target accept --to {b}")
    );
}

#[test]
fn hold_alert_names_the_move_and_its_reasons_briefly() {
    let paths = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
    let hold = Hold {
        accepted: "1a2b3c4d".repeat(5),
        observed: "5d6e7f80".repeat(5),
        reasons: vec![
            HoldReason::NotFastForward,
            HoldReason::ControlPaths { paths },
        ],
        since: chrono::Utc::now(),
    };

    let alert = hold_alert("refs/heads/main", &hold);

    assert_eq!(
        alert,
        "Target main held: moved outside loom 1a2b3c4→5d6e7f8 (not a fast-forward; touches \
         a, b, c and 2 more). Merges into main wait. Review: loom target status"
    );
}

#[test]
fn grafts_do_not_change_ancestry() {
    let repo = repo();
    let a = tip(&repo.root);
    let x = crafted(&repo.root, &a, "src/x.rs", false);
    std::fs::write(repo.root.join(".git/info/grafts"), format!("{x} {a}\n")).unwrap();

    assert!(!is_ancestor_of(&a, &x, &repo.root).unwrap());
}

#[test]
fn replace_refs_do_not_change_the_changed_paths() {
    let repo = repo();
    let a = tip(&repo.root);
    let x = crafted(&repo.root, &a, "src/x.rs", true);
    let d = crafted(&repo.root, &a, "doc/loom/knowledge/decoy.md", true);
    git(&repo.root, &["replace", &x, &d]);

    assert_eq!(changed_paths(&repo.root, &a, &x).unwrap(), vec!["src/x.rs"]);
}
