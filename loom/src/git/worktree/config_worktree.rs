//! The `config.worktree` file of a linked worktree's administrative directory.
//!
//! When the repository's common configuration sets `extensions.worktreeConfig`,
//! git also reads `<common dir>/worktrees/<name>/config.worktree` for that
//! worktree. A session's sandbox may write the git common directory, so an
//! agent can put `filter.<name>.clean` (or `core.fsmonitor`, `core.pager`,
//! `include.path`) there and have git run a command the next time the daemon
//! runs it in that worktree. Two layers close this: the session capsule denies
//! writes to every existing `config.worktree` ([`worktree_admin_dirs`] lists
//! the directories), and a pinned [`super::WorktreeGit`] refuses to run git
//! while the file holds a key outside [`GIT_WRITTEN_KEYS`]
//! ([`check_worktree_config`]).

use anyhow::{bail, ensure, Context, Result};
use std::path::{Path, PathBuf};

use super::pinned::common_dir;
use crate::git::runner::run_git;

/// The settings git itself writes to `config.worktree` when it creates a
/// worktree (`git worktree add` copies the main worktree's sparse-checkout
/// settings there), lowercased. None of them runs a program.
const GIT_WRITTEN_KEYS: [&str; 3] = [
    "core.sparsecheckout",
    "core.sparsecheckoutcone",
    "index.sparse",
];

/// The administrative directories under `<common dir>/worktrees/` of the
/// repository at `repo_root`: real directories only, no symlinks. A directory
/// with no `.git` has no worktrees.
pub fn worktree_admin_dirs(repo_root: &Path) -> Result<Vec<PathBuf>> {
    if !repo_root.join(".git").exists() {
        return Ok(Vec::new());
    }
    let registry = common_dir(repo_root)?.join("worktrees");
    let entries = match std::fs::read_dir(&registry) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("cannot list {}", registry.display()))
        }
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot list {}", registry.display()))?;
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Refuse, unless git ignores or only git wrote `<admin>/config.worktree`.
///
/// Nothing is read when the file is absent, is an empty regular file (the
/// placeholder the sandbox leaves for a denied path; a symlink never takes this
/// shortcut), or the repository at `repo_root` leaves
/// `extensions.worktreeConfig` off (git ignores the file then). The
/// file's keys are listed as data with `git config --file`, which runs
/// nothing, in the main repository; any key outside [`GIT_WRITTEN_KEYS`], or
/// a file git cannot parse, is an error.
pub(super) fn check_worktree_config(repo_root: &Path, admin: &Path) -> Result<()> {
    let file = admin.join("config.worktree");
    match std::fs::symlink_metadata(&file) {
        Err(_) => return Ok(()),
        Ok(meta) if meta.is_file() && meta.len() == 0 => return Ok(()),
        Ok(_) => {}
    }
    if !worktree_config_enabled(repo_root)? {
        return Ok(());
    }
    let path = file
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", file.display()))?;
    let listed = run_git(
        &["config", "--file", path, "--name-only", "--list"],
        repo_root,
    )?;
    ensure!(
        listed.status.success(),
        "loom will not run git in the worktree: {} cannot be parsed ({})",
        file.display(),
        String::from_utf8_lossy(&listed.stderr).trim()
    );
    let listing = String::from_utf8_lossy(&listed.stdout);
    let offending: Vec<&str> = listing
        .lines()
        .filter(|key| !GIT_WRITTEN_KEYS.contains(&key.to_lowercase().as_str()))
        .collect();
    ensure!(
        offending.is_empty(),
        "loom will not run git in the worktree: {} sets {}, which git would act on",
        file.display(),
        offending.join(", ")
    );
    Ok(())
}

/// Whether the repository's common configuration enables
/// `extensions.worktreeConfig`.
fn worktree_config_enabled(repo_root: &Path) -> Result<bool> {
    let output = run_git(
        &[
            "config",
            "--type=bool",
            "--get",
            "extensions.worktreeConfig",
        ],
        repo_root,
    )?;
    match output.status.code() {
        Some(1) => Ok(false),
        Some(0) => Ok(String::from_utf8_lossy(&output.stdout).trim() == "true"),
        _ => bail!(
            "cannot read the repository configuration: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

#[cfg(test)]
#[path = "config_worktree_tests.rs"]
mod tests;
