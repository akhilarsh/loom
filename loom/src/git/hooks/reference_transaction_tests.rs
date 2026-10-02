//! Tests for the `reference-transaction` hook installer.

use super::reference_transaction::SCRIPT;
use super::{
    install_reference_transaction_hook, is_reference_transaction_hook_installed, HookInstall,
};
use crate::git::target_guard::HOOK_MARKER;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A directory with an empty `.git` directory, and the hook's path in it.
fn repo() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join(".git")).unwrap();
    let hook = temp.path().join(".git/hooks/reference-transaction");
    (temp, hook)
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn script_carries_the_hook_marker() {
    assert!(SCRIPT.contains(HOOK_MARKER));
}

#[test]
fn installs_the_hook_when_absent() {
    let (temp, hook) = repo();

    let installed = install_reference_transaction_hook(temp.path()).unwrap();

    assert_eq!(installed, HookInstall::Installed);
    assert_eq!(fs::read_to_string(&hook).unwrap(), SCRIPT);
    assert_eq!(mode(&hook), 0o755);
    assert!(is_reference_transaction_hook_installed(temp.path()));
}

#[test]
fn second_install_is_up_to_date() {
    let (temp, _hook) = repo();
    install_reference_transaction_hook(temp.path()).unwrap();

    let again = install_reference_transaction_hook(temp.path()).unwrap();

    assert_eq!(again, HookInstall::UpToDate);
}

#[test]
fn overwrites_an_older_loom_hook() {
    let (temp, hook) = repo();
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, format!("#!/bin/sh\n# {HOOK_MARKER}\nexit 0\n")).unwrap();

    let installed = install_reference_transaction_hook(temp.path()).unwrap();

    assert_eq!(installed, HookInstall::Installed);
    assert_eq!(fs::read_to_string(&hook).unwrap(), SCRIPT);
    assert_eq!(mode(&hook), 0o755);
}

#[test]
fn restores_the_mode_of_a_current_hook() {
    let (temp, hook) = repo();
    install_reference_transaction_hook(temp.path()).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o644)).unwrap();

    let installed = install_reference_transaction_hook(temp.path()).unwrap();

    assert_eq!(installed, HookInstall::Installed);
    assert_eq!(mode(&hook), 0o755);
}

#[test]
fn foreign_hook_is_left_untouched() {
    let (temp, hook) = repo();
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    let foreign = b"#!/bin/sh\n# another tool's hook\n\xff\xfe\nexit 0\n";
    fs::write(&hook, foreign).unwrap();

    let installed = install_reference_transaction_hook(temp.path()).unwrap();

    assert_eq!(installed, HookInstall::ForeignHookPresent);
    assert_eq!(fs::read(&hook).unwrap(), foreign);
    assert!(!is_reference_transaction_hook_installed(temp.path()));
}

#[test]
fn refuses_without_a_git_directory() {
    let temp = TempDir::new().unwrap();

    let result = install_reference_transaction_hook(temp.path());

    assert!(result.is_err());
    assert!(!temp.path().join(".git").exists());
    assert!(!is_reference_transaction_hook_installed(temp.path()));
}
