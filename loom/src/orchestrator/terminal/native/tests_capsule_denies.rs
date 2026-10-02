//! The state-directory and `config.worktree` denies of every session kind's
//! capsule, in both layers (`sandbox.filesystem.denyWrite` for Bash,
//! `permissions.deny` for the native file tools).

use super::*;
use crate::git::runner::run_git;
use crate::models::session::SessionType;
use crate::orchestrator::terminal::native::tests_session_settings::{
    checkout, read_capsule, sandbox_with, strings, write_capsule,
};

const DENY_WRITE: &str = "/sandbox/filesystem/denyWrite";
const DENY_RULES: &str = "/permissions/deny";

/// Every session kind with the directory it runs in: merge resolvers run in
/// the stage worktree, knowledge and adjudication sessions in the checkout.
fn kinds_with_cwd(repo: &Path, worktree: &Path) -> [(SessionType, PathBuf); 6] {
    [
        (SessionType::Stage, worktree.to_path_buf()),
        (SessionType::Contract, worktree.to_path_buf()),
        (SessionType::Merge, worktree.to_path_buf()),
        (SessionType::Knowledge, repo.to_path_buf()),
        (SessionType::Adjudication, repo.to_path_buf()),
        (SessionType::BaseConflict, repo.to_path_buf()),
    ]
}

fn git(dir: &Path, args: &[&str]) {
    let output = run_git(args, dir).unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// Make `repo` a git repository with the registered worktrees `s1` and `s2`.
fn register_worktrees(repo: &Path) {
    git(repo, &["init", "-q", "-b", "main"]);
    git(
        repo,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "base",
        ],
    );
    git(
        repo,
        &["worktree", "add", "-q", "-b", "loom/s1", ".worktrees/s1"],
    );
    git(
        repo,
        &["worktree", "add", "-q", "-b", "loom/s2", ".worktrees/s2"],
    );
}

/// `human-review --force-complete` has no operator proof of its own: it rests
/// on no session being able to write the stage file it changes.
#[test]
fn every_session_kind_denies_writes_to_the_state_directory() {
    let checkout = checkout();
    let sandbox = sandbox_with(vec![]);
    let state = checkout.repo.join(".loom");
    for (kind, cwd) in kinds_with_cwd(&checkout.repo, &checkout.worktree) {
        let path = write_capsule(
            &checkout,
            "session-st1",
            kind,
            &cwd,
            &sandbox,
            Some(&checkout.hooks_dir),
        )
        .unwrap();
        let capsule = read_capsule(&path);

        let deny_write = strings(&capsule, DENY_WRITE);
        let rules = strings(&capsule, DENY_RULES);
        assert!(
            deny_write.contains(state.to_str().unwrap()),
            "{kind}: {capsule}"
        );
        assert!(
            rules.contains(&format!("Edit(/{}/**)", state.display())),
            "{kind}: {capsule}"
        );
        assert!(rules.contains("Edit(.loom/**)"), "{kind}: {capsule}");
    }
}

#[test]
fn every_session_kind_denies_every_worktrees_config_worktree() {
    let checkout = checkout();
    let sandbox = sandbox_with(vec![]);
    register_worktrees(&checkout.repo);
    for (kind, cwd) in kinds_with_cwd(&checkout.repo, &checkout.worktree) {
        let path = write_capsule(
            &checkout,
            "session-cw1",
            kind,
            &cwd,
            &sandbox,
            Some(&checkout.hooks_dir),
        )
        .unwrap();
        let capsule = read_capsule(&path);

        for name in ["s1", "s2"] {
            let file = checkout
                .repo
                .join(".git/worktrees")
                .join(name)
                .join("config.worktree");
            let file = file.to_str().unwrap();
            assert!(
                strings(&capsule, DENY_WRITE).contains(file),
                "{kind}: {capsule}"
            );
            assert!(
                strings(&capsule, DENY_RULES).contains(&format!("Edit(/{file})")),
                "{kind}: {capsule}"
            );
        }
    }
}
