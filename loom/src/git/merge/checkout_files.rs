//! Reads and writes the operator's checkout for the off-checkout merge:
//! the status and diff inputs of [`super::checkout_state::classify`], the disk
//! probe behind it, and removing and restoring the untracked files a merge
//! replaces.

use anyhow::{bail, Context, Result};
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::checkout_state::{CheckoutProbe, DiskEntry};
use crate::git::runner::{run_git, run_git_with_env};

/// Stdout of a git command that must succeed, untrimmed (`-z` output).
pub(super) fn checked_stdout(
    args: &[&str],
    env: &[(&str, &OsStr)],
    repo: &Path,
) -> Result<Vec<u8>> {
    let output = run_git_with_env(args, env, repo)?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

/// `git status --porcelain=v2 -z --untracked-files=all` without taking
/// `.git/index.lock`, which would block the operator's own git.
pub(super) fn porcelain_status(repo: &Path) -> Result<Vec<u8>> {
    checked_stdout(
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        &[("GIT_OPTIONAL_LOCKS", OsStr::new("0"))],
        repo,
    )
}

/// Reads the checkout at `repo` against the merge `base` -> `tree`.
pub(super) struct DiskProbe<'a> {
    pub repo: &'a Path,
    pub base: &'a str,
    pub tree: &'a str,
}

impl CheckoutProbe for DiskProbe<'_> {
    fn disk(&self, path: &str) -> DiskEntry {
        match std::fs::symlink_metadata(self.repo.join(path)) {
            Ok(meta) if meta.is_file() => DiskEntry::RegularFile,
            Ok(meta) if meta.is_dir() => DiskEntry::Directory,
            Ok(_) => DiskEntry::Other,
            // A parent that is a file leaves nothing at the path itself.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                DiskEntry::Absent
            }
            // An unreadable path cannot be proven free: treat it as occupied.
            Err(_) => DiskEntry::Other,
        }
    }

    fn equals_blob(&self, path: &str) -> bool {
        let is_regular = tree_entry_mode(self.repo, self.tree, path)
            .is_some_and(|mode| mode == "100644" || mode == "100755");
        if !is_regular || self.disk(path) != DiskEntry::RegularFile {
            return false;
        }
        let Some(blob) = read_blob(self.repo, self.tree, path) else {
            return false;
        };
        std::fs::read(self.repo.join(path)).is_ok_and(|bytes| bytes == blob)
    }

    fn in_base_tree(&self, path: &str) -> bool {
        tree_entry_mode(self.repo, self.base, path).is_some()
    }
}

/// `ls-tree -z` prints `<mode> <type> <oid>\t<path>`; the mode of the entry
/// at `path` in `rev`, if it exists.
fn tree_entry_mode(repo: &Path, rev: &str, path: &str) -> Option<String> {
    let output = run_git(&["ls-tree", "-z", rev, "--", path], repo).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (mode, _) = text.split_once(' ')?;
    (!mode.is_empty()).then(|| mode.to_string())
}

fn read_blob(repo: &Path, rev: &str, path: &str) -> Option<Vec<u8>> {
    let blob = run_git(&["cat-file", "blob", &format!("{rev}:{path}")], repo).ok()?;
    blob.status.success().then_some(blob.stdout)
}

/// Remove `paths`, whose bytes equal their blobs in `rev`. When a removal
/// fails, the files already removed are written back and the error returned.
pub(super) fn remove_files(repo: &Path, rev: &str, paths: &[String]) -> Result<()> {
    for (index, path) in paths.iter().enumerate() {
        if let Err(error) = std::fs::remove_file(repo.join(path)) {
            restore_files(repo, rev, &paths[..index]);
            return Err(error).with_context(|| format!("Cannot remove {path}"));
        }
    }
    Ok(())
}

/// Write back untracked files removed before a refused fast-forward: the
/// bytes and the exec bit of the blob in `rev`.
pub(super) fn restore_files(repo: &Path, rev: &str, paths: &[String]) {
    for path in paths {
        let Some(blob) = read_blob(repo, rev, path) else {
            continue;
        };
        let full = repo.join(path);
        if std::fs::write(&full, blob).is_err() {
            continue;
        }
        if tree_entry_mode(repo, rev, path).is_some_and(|mode| mode == "100755") {
            let _ = std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o755));
        }
    }
}

#[cfg(test)]
mod tests;
