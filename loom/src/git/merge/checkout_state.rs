//! Classify the uncommitted state of the operator's checkout against a merge.
//!
//! Inputs are `git status --porcelain=v2 -z --untracked-files=all` run in the
//! checkout and `git diff --name-status -z --no-renames <old> <new>`. A path
//! is "touched" when `old..new` modifies, adds or deletes it. Everything here
//! is pure; the byte comparison for untracked files is a parameter.

use std::collections::BTreeSet;

/// What to do with the checkout's uncommitted state before a fast-forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// No tracked change and no untracked file is on a touched path; git
    /// carries the local changes through the fast-forward.
    NoOverlap,
    /// The only overlap is untracked files the merge adds with identical
    /// bytes; remove them, then fast-forward.
    RemoveUntracked { paths: Vec<String> },
    /// A tracked change sits on a touched path. The caller dry-runs the
    /// reapply to tell a clean overlap from a conflict.
    Reapply {
        tracked: Vec<String>,
        remove_untracked: Vec<String>,
    },
    /// Refuse: an untracked file with different content on a touched path,
    /// or an unmerged index entry (`merge --ff-only` refuses those).
    Blocked { paths: Vec<String> },
}

/// Parsed `status --porcelain=v2 -z` output.
#[derive(Debug, Default, PartialEq, Eq)]
struct StatusEntries {
    tracked: BTreeSet<String>,
    unmerged: Vec<String>,
    untracked: Vec<String>,
}

/// Classify the checkout. `untracked_equal(path)` answers whether the
/// untracked file at `path` has exactly the bytes the merge would write.
pub fn classify(
    status_v2_z: &str,
    diff_name_status_z: &str,
    untracked_equal: impl Fn(&str) -> bool,
) -> Classification {
    let status = parse_status(status_v2_z);
    if !status.unmerged.is_empty() {
        return Classification::Blocked {
            paths: status.unmerged,
        };
    }
    let touched = parse_touched(diff_name_status_z);

    let mut remove_untracked = Vec::new();
    let mut differing = Vec::new();
    for path in status.untracked.iter().filter(|p| touched.contains(*p)) {
        if untracked_equal(path) {
            remove_untracked.push(path.clone());
        } else {
            differing.push(path.clone());
        }
    }
    if !differing.is_empty() {
        return Classification::Blocked { paths: differing };
    }

    let tracked: Vec<String> = status
        .tracked
        .into_iter()
        .filter(|p| touched.contains(p))
        .collect();
    match (tracked.is_empty(), remove_untracked.is_empty()) {
        (true, true) => Classification::NoOverlap,
        (true, false) => Classification::RemoveUntracked {
            paths: remove_untracked,
        },
        (false, _) => Classification::Reapply {
            tracked,
            remove_untracked,
        },
    }
}

/// Paths from `status --porcelain=v2 -z`. Fixed field counts with `splitn`
/// keep paths containing spaces intact; a rename (`2`) carries its original
/// path in the next NUL field, and both count as tracked changes.
fn parse_status(status: &str) -> StatusEntries {
    let mut entries = StatusEntries::default();
    let mut fields = status.split('\0');
    while let Some(entry) = fields.next() {
        match entry.chars().next() {
            Some('1') => push_path(&mut entries.tracked, entry, 9),
            Some('2') => {
                push_path(&mut entries.tracked, entry, 10);
                if let Some(orig) = fields.next().filter(|o| !o.is_empty()) {
                    entries.tracked.insert(orig.to_string());
                }
            }
            Some('u') => {
                if let Some(path) = entry.splitn(11, ' ').nth(10) {
                    entries.unmerged.push(path.to_string());
                }
            }
            Some('?') => {
                if let Some(path) = entry.strip_prefix("? ") {
                    entries.untracked.push(path.to_string());
                }
            }
            _ => {}
        }
    }
    entries
}

fn push_path(into: &mut BTreeSet<String>, entry: &str, field_count: usize) {
    if let Some(path) = entry.splitn(field_count, ' ').nth(field_count - 1) {
        into.insert(path.to_string());
    }
}

/// Paths from `diff --name-status -z --no-renames`: `<status>\0<path>\0` pairs.
fn parse_touched(diff: &str) -> BTreeSet<String> {
    let mut fields = diff.split('\0').filter(|f| !f.is_empty());
    let mut touched = BTreeSet::new();
    while let (Some(_status), Some(path)) = (fields.next(), fields.next()) {
        touched.insert(path.to_string());
    }
    touched
}

#[cfg(test)]
mod tests;
