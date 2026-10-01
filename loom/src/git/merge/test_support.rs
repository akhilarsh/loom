//! Shared helpers for the merge tests: scratch repositories with isolated git
//! config, branch builders, and a snapshot of a checkout's visible state.

use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub(super) fn isolated_git(root: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", root.join(".loom-test-no-global"))
        .env("GIT_CONFIG_SYSTEM", root.join(".loom-test-no-system"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

pub(super) fn git_ok(root: &Path, args: &[&str]) {
    let out = isolated_git(root, args);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Trimmed stdout of a git command that must succeed.
pub(super) fn git_out(root: &Path, args: &[&str]) -> String {
    let out = isolated_git(root, args);
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Repository on `main` with `a.txt` ("seed") committed.
pub(super) fn init_repo() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git_ok(root, &["init", "-b", "main"]);
    git_ok(root, &["config", "user.email", "t@t.com"]);
    git_ok(root, &["config", "user.name", "t"]);
    commit_file(root, "a.txt", "seed", "seed");
    temp
}

/// A work directory outside the repository, so the merge lock file does not
/// show up as an untracked file in it.
pub(super) fn lock_dir() -> TempDir {
    TempDir::new().unwrap()
}

/// Write `name`, stage it and commit on the current branch.
pub(super) fn commit_file(root: &Path, name: &str, text: &str, message: &str) {
    std::fs::write(root.join(name), text).unwrap();
    git_ok(root, &["add", name]);
    git_ok(root, &["commit", "-m", message]);
}

/// Create `loom/<id>` from `main` with one commit per `(file, text)` pair,
/// then return to `main`.
pub(super) fn stage_branch(root: &Path, id: &str, files: &[(&str, &str)]) {
    git_ok(root, &["checkout", "-b", &format!("loom/{id}"), "main"]);
    for (name, text) in files {
        commit_file(root, name, text, &format!("stage {name}"));
    }
    git_ok(root, &["checkout", "main"]);
}

pub(super) fn rev(root: &Path, name: &str) -> String {
    git_out(root, &["rev-parse", name])
}

/// Everything an operator can see in a checkout: porcelain status plus the
/// bytes of every tracked or untracked file.
pub(super) fn snapshot(root: &Path) -> (String, Vec<(String, Vec<u8>)>) {
    let status = git_out(root, &["status", "--porcelain=v2", "--untracked-files=all"]);
    let listing = isolated_git(root, &["ls-files", "-z", "--cached", "--others"]);
    let files = String::from_utf8_lossy(&listing.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(|p| {
            (
                p.to_string(),
                std::fs::read(root.join(p)).unwrap_or_default(),
            )
        })
        .collect();
    (status, files)
}

/// A sibling directory for a second worktree.
pub(super) fn sibling_dir() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("second");
    (temp, path)
}
