//! Contracts for `loom target status` and `loom target accept`: the operator
//! reviews a move of `main` loom did not make and accepts the current tip or
//! restores the accepted one. Every command runs the built binary from the
//! root of a scratch repository holding `.loom/work/` (no `config.toml`, so
//! the target is `main`).

#[path = "integration/helpers.rs"]
#[allow(dead_code)]
mod helpers;

use loom::git::target_guard::{accepted_tip, check, recorded_hold, GuardState};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("LOOM_SESSION_ID")
        .env("GIT_CONFIG_GLOBAL", dir.join(".loom-test-no-global"))
        .env("GIT_CONFIG_SYSTEM", dir.join(".loom-test-no-system"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn main_tip(root: &Path) -> String {
    git(root, &["rev-parse", "main"])
}

/// A repository on `main` with one commit and `.loom/work/` created; the
/// guard records `main`, then an operator commit adds
/// `.claude/settings.json` and the guard holds it. Returns the repository,
/// its root, the work dir, the accepted tip and the moved tip.
fn held_repo() -> (TempDir, PathBuf, PathBuf, String, String) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join(".git/info/exclude"), ".loom/\n.worktrees/\n").unwrap();
    std::fs::write(root.join("README.md"), "readme\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-q", "-m", "seed"]);
    let work_dir = root.join(".loom/work");
    std::fs::create_dir_all(&work_dir).unwrap();
    let first = check(&root, &work_dir, "main").unwrap();
    assert!(matches!(first, Some(GuardState::Clear { .. })));
    let accepted = main_tip(&root);

    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude/settings.json"), "{}\n").unwrap();
    git(&root, &["add", ".claude/settings.json"]);
    git(&root, &["commit", "-q", "-m", "settings"]);
    let moved = main_tip(&root);
    let second = check(&root, &work_dir, "main").unwrap();
    assert!(matches!(second, Some(GuardState::Held(_))), "{second:?}");
    (dir, root, work_dir, accepted, moved)
}

fn loom(root: &Path, args: &[&str]) -> Output {
    helpers::loom_cmd()
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn target_accept_with_current_tip_clears_the_hold() {
    let (_dir, root, work_dir, _accepted, moved) = held_repo();

    let out = loom(&root, &["target", "accept", "--to", &moved]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(recorded_hold(&work_dir, "main").unwrap(), None);
    assert_eq!(accepted_tip(&work_dir, "main").unwrap(), Some(moved));
}

#[test]
fn target_accept_with_stale_tip_is_refused() {
    let (_dir, root, work_dir, accepted, _moved) = held_repo();

    let out = loom(&root, &["target", "accept", "--to", &accepted]);

    assert!(!out.status.success(), "a stale --to was accepted");
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("the target moved since you reviewed it"),
        "stderr: {stderr}"
    );
    assert!(recorded_hold(&work_dir, "main").unwrap().is_some());
    assert_eq!(accepted_tip(&work_dir, "main").unwrap(), Some(accepted));
}

#[test]
fn target_status_prints_review_and_restore_commands() {
    let (_dir, root, _work_dir, accepted, moved) = held_repo();
    assert_eq!(git(&root, &["symbolic-ref", "HEAD"]), "refs/heads/main");

    let out = loom(&root, &["target", "status"]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let expected = [
        "Attestation: off (".to_string(),
        format!("loom target accept --to {moved}"),
        format!("git update-ref refs/heads/main {accepted} {moved}"),
        format!("git read-tree -m -u {moved} {accepted}"),
    ];
    for line in &expected {
        assert!(
            stdout.contains(line.as_str()),
            "missing {line:?} in:\n{stdout}"
        );
    }
}
