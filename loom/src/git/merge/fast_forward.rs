//! `merge --ff-only` in the operator's checkout, and the stash restore around
//! it. A refused fast-forward is always a typed block: everything this call
//! changed is put back first, and the operator's work is never left only in
//! the stash without saying so.

use anyhow::Result;
use std::path::Path;

use super::checkout_files::restore_files;
use super::tree::{rev_parse, Advance, MergeBlock, StashReapply};
use crate::git::runner::run_git;

/// Longest `FastForwardRefused` detail, in characters.
const DETAIL_LIMIT: usize = 300;

/// Headers git prints before the tab-indented paths of a refused merge.
const OVERLAP_HEADERS: [&str; 4] = [
    "Your local changes to the following files would be overwritten by merge:",
    "The following untracked working tree files would be overwritten by merge:",
    "The following untracked working tree files would be removed by merge:",
    "Updating the following directories would lose untracked files in them:",
];

/// `merge --ff-only` from `old` to `new`, then restore a stash made for it
/// (when `backup_ref` is set). On a refused fast-forward the stash is popped
/// and `removed` is written back.
pub(super) fn fast_forward(
    repo: &Path,
    (old, new): (&str, &str),
    removed: &[String],
    backup_ref: Option<String>,
) -> Result<Advance> {
    if let Err(stderr) = try_fast_forward(repo, new)? {
        let stash_restored = match backup_ref {
            Some(_) => pop_stash(repo),
            None => true,
        };
        restore_files(repo, new, removed);
        if let (false, Some(backup_ref)) = (stash_restored, backup_ref) {
            return Ok(Advance::Blocked(MergeBlock::StashNotRestored {
                backup_ref,
            }));
        }
        if rev_parse(repo, "HEAD")? != old {
            return Ok(Advance::Blocked(MergeBlock::TargetMoved));
        }
        return Ok(Advance::Blocked(refused_block(&stderr)));
    }
    let stash = backup_ref.map(|backup_ref| StashReapply {
        restored: pop_stash(repo),
        backup_ref,
    });
    Ok(Advance::Advanced { stash })
}

/// `Ok(Err(stderr))` when git refused the fast-forward.
fn try_fast_forward(repo: &Path, new: &str) -> Result<std::result::Result<(), String>> {
    #[cfg(test)]
    if failpoint::fast_forward() {
        return Ok(Err("simulated refusal".to_string()));
    }
    let output = run_git(&["merge", "--ff-only", "--quiet", new], repo)?;
    if output.status.success() {
        return Ok(Ok(()));
    }
    Ok(Err(String::from_utf8_lossy(&output.stderr).into_owned()))
}

/// `stash pop --index`, then plain `stash pop`. Returns whether either
/// worked; on failure the stash entry stays.
fn pop_stash(repo: &Path) -> bool {
    #[cfg(test)]
    if failpoint::pop() {
        return false;
    }
    let attempts: [&[&str]; 2] = [
        &["stash", "pop", "--index", "--quiet"],
        &["stash", "pop", "--quiet"],
    ];
    attempts
        .iter()
        .any(|args| run_git(args, repo).is_ok_and(|o| o.status.success()))
}

/// The block for a refused fast-forward whose stderr is `stderr`: the paths
/// git names, or the flattened message when it names none.
pub(super) fn refused_block(stderr: &str) -> MergeBlock {
    let mut paths = Vec::new();
    let mut listing = false;
    for line in stderr.lines() {
        if OVERLAP_HEADERS.iter().any(|header| line.contains(header)) {
            listing = true;
        } else if let Some(path) = line.strip_prefix('\t').filter(|_| listing) {
            paths.push(path.trim_end().to_string());
        } else {
            listing = false;
        }
    }
    if !paths.is_empty() {
        return MergeBlock::UncommittedOverlap { paths };
    }
    let flat = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    MergeBlock::FastForwardRefused {
        detail: flat.chars().take(DETAIL_LIMIT).collect(),
    }
}

/// Test-only switches that make the fast-forward or the stash pop fail on
/// the current thread, to reach paths git does not fail on by itself.
#[cfg(test)]
pub(super) mod failpoint {
    use std::cell::Cell;

    #[derive(Clone, Copy, Default)]
    pub struct Failures {
        pub fast_forward: bool,
        pub pop: bool,
    }

    thread_local! {
        static FAILURES: Cell<Failures> = const { Cell::new(Failures { fast_forward: false, pop: false }) };
    }

    /// Resets the switches when dropped.
    pub struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            FAILURES.with(|cell| cell.set(Failures::default()));
        }
    }

    pub fn inject(failures: Failures) -> Guard {
        FAILURES.with(|cell| cell.set(failures));
        Guard
    }

    pub(super) fn fast_forward() -> bool {
        FAILURES.with(|cell| cell.get().fast_forward)
    }

    pub(super) fn pop() -> bool {
        FAILURES.with(|cell| cell.get().pop)
    }
}

#[cfg(test)]
mod tests;
