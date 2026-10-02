//! The reference-transaction hook arm of `loom repair`.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::{check, REFERENCE_TRANSACTION_ISSUE};
use crate::commands::repair::workspace::WorkspaceFix;
use crate::git::hooks::{
    install_reference_transaction_hook, is_reference_transaction_hook_installed,
};

const FOREIGN_HOOK: &str = "#!/bin/sh\necho another tool\n";

/// A directory with `.git/hooks` and no hooks in it.
fn repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".git/hooks")).unwrap();
    dir
}

fn hook_path(root: &Path) -> PathBuf {
    root.join(".git/hooks/reference-transaction")
}

/// The descriptions of the reference-transaction issues `check` reports.
fn hook_issues(root: &Path) -> Vec<String> {
    check(root)
        .into_iter()
        .map(|issue| issue.description)
        .filter(|description| description.starts_with(REFERENCE_TRANSACTION_ISSUE))
        .collect()
}

#[test]
fn a_missing_hook_is_reported_and_repair_installs_it() {
    let repo = repo();
    let root = repo.path();
    assert_eq!(
        hook_issues(root),
        ["Git reference-transaction hook not installed"]
    );

    let fix = WorkspaceFix::classify("Git reference-transaction hook not installed")
        .expect("the missing-hook issue must classify as a workspace fix");

    assert!(fix.apply(root, false).unwrap());
    assert!(is_reference_transaction_hook_installed(root));
    assert!(hook_issues(root).is_empty());
    assert!(
        !fix.apply(root, false).unwrap(),
        "a second repair finds the hook in place"
    );
}

#[test]
fn a_foreign_hook_is_reported_and_never_overwritten() {
    let repo = repo();
    let root = repo.path();
    fs::write(hook_path(root), FOREIGN_HOOK).unwrap();
    let description = "Git reference-transaction hook belongs to another tool";
    assert_eq!(hook_issues(root), [description]);

    let fix = WorkspaceFix::classify(description)
        .expect("the foreign-hook issue must classify as a workspace fix");
    let error = fix.apply(root, true).unwrap_err().to_string();

    assert!(error.contains("belongs to another tool"), "{error}");
    assert_eq!(fs::read_to_string(hook_path(root)).unwrap(), FOREIGN_HOOK);
}

#[test]
fn a_loom_hook_raises_no_issue() {
    let repo = repo();
    install_reference_transaction_hook(repo.path()).unwrap();

    assert!(hook_issues(repo.path()).is_empty());
}

#[test]
fn outside_a_repository_there_is_no_hook_issue() {
    let dir = TempDir::new().unwrap();

    assert!(hook_issues(dir.path()).is_empty());
}
