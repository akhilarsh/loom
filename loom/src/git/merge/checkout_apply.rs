//! Fast-forward the branch checked out in the operator's checkout while
//! keeping that checkout's uncommitted work.
//!
//! Git carries local changes through `merge --ff-only` when none sits on a
//! path the merge touches. When one does, the tracked changes are stashed
//! around the fast-forward after a dry run proves they reapply cleanly.
//! `reset --hard` and `--autostash` are never used: the latter writes
//! conflict markers into working files.

use anyhow::{bail, Result};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::checkout_state::{classify, Classification};
use super::tree::{rev_parse, Advance, MergeBlock};
use crate::git::runner::{run_git, run_git_checked};

/// Fast-forward the checked-out branch from `old` to `new`.
pub(super) fn advance_in_checkout(
    repo: &Path,
    old: &str,
    new: &str,
    stage_id: &str,
) -> Result<Advance> {
    if rev_parse(repo, "HEAD")? != old {
        return Ok(Advance::Blocked(MergeBlock::TargetMoved));
    }
    let status = run_git(
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        repo,
    )?;
    let diff = run_git(
        &["diff", "--name-status", "-z", "--no-renames", old, new],
        repo,
    )?;
    let status = String::from_utf8_lossy(&status.stdout);
    let diff = String::from_utf8_lossy(&diff.stdout);
    let classification = classify(&status, &diff, |path| {
        untracked_equals_blob(repo, new, path)
    });

    match classification {
        Classification::NoOverlap => fast_forward(repo, old, new, &[], None),
        Classification::RemoveUntracked { paths } => {
            remove_files(repo, &paths)?;
            fast_forward(repo, old, new, &paths, None)
        }
        Classification::Blocked { paths } => Ok(overlap(paths)),
        Classification::Reapply {
            tracked,
            remove_untracked,
        } => reapply(repo, (old, new), stage_id, tracked, remove_untracked),
    }
}

fn overlap(paths: Vec<String>) -> Advance {
    Advance::Blocked(MergeBlock::UncommittedOverlap { paths })
}

/// Dry run, then stash, fast-forward and pop. `S` is `git stash create`, a
/// commit of the tracked changes that touches nothing in the working tree.
fn reapply(
    repo: &Path,
    (old, new): (&str, &str),
    stage_id: &str,
    tracked: Vec<String>,
    remove_untracked: Vec<String>,
) -> Result<Advance> {
    let stash_commit = run_git_checked(&["stash", "create"], repo)?;
    if stash_commit.is_empty() {
        remove_files(repo, &remove_untracked)?;
        return fast_forward(repo, old, new, &remove_untracked, None);
    }
    let base = format!("--merge-base={old}");
    let dry = run_git(
        &["merge-tree", "--write-tree", &base, new, &stash_commit],
        repo,
    )?;
    if !dry.status.success() {
        return Ok(overlap(tracked));
    }

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let backup_ref = format!("refs/loom/autostash/{stage_id}-{secs}");
    run_git_checked(&["update-ref", &backup_ref, &stash_commit], repo)?;
    remove_files(repo, &remove_untracked)?;
    if let Err(error) = run_git_checked(&["stash", "push", "--quiet"], repo) {
        restore_files(repo, new, &remove_untracked);
        return Err(error);
    }
    fast_forward(repo, old, new, &remove_untracked, Some(backup_ref))
}

/// `merge --ff-only new`, then restore a stash made for it (when
/// `backup_ref` is set). On a refused fast-forward everything this call
/// changed is put back.
fn fast_forward(
    repo: &Path,
    old: &str,
    new: &str,
    removed: &[String],
    backup_ref: Option<String>,
) -> Result<Advance> {
    let output = run_git(&["merge", "--ff-only", "--quiet", new], repo)?;
    if !output.status.success() {
        if backup_ref.is_some() {
            pop_stash(repo);
        }
        restore_files(repo, new, removed);
        if rev_parse(repo, "HEAD")? != old {
            return Ok(Advance::Blocked(MergeBlock::TargetMoved));
        }
        bail!(
            "git merge --ff-only {new} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    match backup_ref {
        Some(backup_ref) if !pop_stash(repo) => {
            Ok(Advance::Blocked(MergeBlock::ReapplyFailed { backup_ref }))
        }
        backup_ref => Ok(Advance::Advanced { backup_ref }),
    }
}

/// `stash pop --index`, then plain `stash pop`. Returns whether either
/// worked; on failure the stash entry stays.
fn pop_stash(repo: &Path) -> bool {
    let attempts: [&[&str]; 2] = [
        &["stash", "pop", "--index", "--quiet"],
        &["stash", "pop", "--quiet"],
    ];
    attempts
        .iter()
        .any(|args| run_git(args, repo).is_ok_and(|o| o.status.success()))
}

fn remove_files(repo: &Path, paths: &[String]) -> Result<()> {
    for path in paths {
        std::fs::remove_file(repo.join(path))?;
    }
    Ok(())
}

/// Write back untracked files removed before a refused fast-forward; their
/// bytes equal the blob in `rev`.
fn restore_files(repo: &Path, rev: &str, paths: &[String]) {
    for path in paths {
        if let Ok(blob) = run_git(&["cat-file", "blob", &format!("{rev}:{path}")], repo) {
            if blob.status.success() {
                let _ = std::fs::write(repo.join(path), blob.stdout);
            }
        }
    }
}

/// True when the untracked file at `path` is a regular file (never a
/// symlink) whose bytes equal the regular blob the merge adds there.
fn untracked_equals_blob(repo: &Path, rev: &str, path: &str) -> bool {
    let full = repo.join(path);
    let is_regular = std::fs::symlink_metadata(&full).is_ok_and(|m| m.is_file());
    if !is_regular || !tree_entry_is_regular(repo, rev, path) {
        return false;
    }
    let Ok(blob) = run_git(&["cat-file", "blob", &format!("{rev}:{path}")], repo) else {
        return false;
    };
    blob.status.success() && std::fs::read(&full).is_ok_and(|bytes| bytes == blob.stdout)
}

/// `ls-tree -z` prints `<mode> blob <oid>\t<path>`; regular files are 100644
/// or 100755.
fn tree_entry_is_regular(repo: &Path, rev: &str, path: &str) -> bool {
    run_git(&["ls-tree", "-z", rev, "--", path], repo).is_ok_and(|o| {
        let text = String::from_utf8_lossy(&o.stdout);
        o.status.success() && text.starts_with("100")
    })
}
