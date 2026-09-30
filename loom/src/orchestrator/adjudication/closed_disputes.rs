//! Closing the disputes a stage abandons when it escalates to a human.
//!
//! An escalation out of the dispute loop leaves the stage's unanswered
//! disputes on disk, and an apply-cap escalation leaves a verdict that never
//! applied. Left open, the oldest of them shadows the dispute a fresh session
//! files once the review is approved: disputes are scanned in ascending id
//! order, so the abandoned one would be judged again, or escalated again at
//! its spent attempt cap, ahead of the new one. A `closed.marker` in the
//! dispute's own directory settles it for good, and [`is_closed`] is the one
//! predicate every reader of dispute state consults.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::os::fd::OwnedFd;
use std::path::Path;

use crate::fs::safe_fs::flock_exclusive;
use crate::models::dispute::{applied_marker, dispute_dir};

use super::scan::dispute_ids;

/// Zero-byte marker written into a closed dispute's directory.
const CLOSED_MARKER: &str = "closed.marker";

/// Whether the dispute whose directory is `dispute_dir` was closed. Any entry
/// under the marker's name counts, so a dangling link cannot reopen it.
pub(super) fn is_closed(dispute_dir: &Path) -> bool {
    std::fs::symlink_metadata(dispute_dir.join(CLOSED_MARKER)).is_ok()
}

/// Take the per-stage lock that serialises dispute filings for the stage
/// whose dispute directory is `stage_dir`, creating the directory and its
/// `.lock` file if needed. The lock is held until the descriptor drops.
pub(crate) fn lock_stage_dispute_dir(stage_dir: &Path) -> Result<OwnedFd> {
    std::fs::create_dir_all(stage_dir)
        .with_context(|| format!("Failed to create {}", stage_dir.display()))?;
    let lock_path = stage_dir.join(".lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("Failed to open {}", lock_path.display()))?;
    let lock = OwnedFd::from(lock_file);
    flock_exclusive(&lock)
        .with_context(|| format!("Failed to acquire dispute lock at {}", lock_path.display()))?;
    Ok(lock)
}

/// Close every dispute of `stage_id` that has no applied outcome, under the
/// lock its filings take.
///
/// Called only once the stage has been escalated to `NeedsHumanReview`: a
/// stage still in the dispute loop must keep its disputes open. Best effort,
/// like the escalation itself: a failure is logged per dispute and the rest
/// are still closed.
pub(crate) fn close_open_disputes(work_dir: &Path, stage_id: &str) {
    let disputes_root = work_dir.join("disputes");
    let stage_dir = disputes_root.join(stage_id);
    let _lock = match lock_stage_dispute_dir(&stage_dir) {
        Ok(lock) => lock,
        Err(error) => {
            warn_not_closed(stage_id, None, &format!("{error:#}"));
            return;
        }
    };
    for id in dispute_ids(&stage_dir) {
        if applied_marker(&disputes_root, stage_id, id).exists() {
            continue;
        }
        let marker = dispute_dir(&disputes_root, stage_id, id).join(CLOSED_MARKER);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
        {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => warn_not_closed(stage_id, Some(id), &error.to_string()),
        }
    }
}

fn warn_not_closed(stage_id: &str, dispute_id: Option<u32>, error: &str) {
    tracing::warn!(
        target: "loom::adjudication",
        stage = %stage_id,
        dispute = ?dispute_id,
        %error,
        "failed to close a dispute of a stage escalated to human review; \
         it may be judged again once the review is approved",
    );
}
