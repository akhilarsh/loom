//! The per-stage merge-resolver attempt counter: written by the daemon's spawn
//! loop, read by it and by `loom status`.

use std::path::{Path, PathBuf};

/// Maximum number of merge-resolver sessions the daemon will spawn for a single
/// stage before giving up and routing it to `NeedsHumanReview`. Mirrors the
/// crash-retry cap (the default `max_retries` of 3).
///
/// Without this cap a resolver that fails fast and deterministically would be
/// respawned on every ~5s poll cycle (the kept signal file is NOT a guard —
/// `find_live_merge_session_for_stage` deletes it once the PID is dead), each
/// spawn on `opus`/`xhigh` → unbounded token + window burn (O-3).
pub(crate) const MAX_MERGE_RESOLVER_ATTEMPTS: u32 = 3;

/// Directory holding per-stage merge-resolver attempt counters.
///
/// Stored on disk (rather than in memory) so the cap survives daemon
/// restarts — a resolver that crash-loops across restarts must not reset
/// its budget each time `loom run` starts.
pub(super) fn attempts_dir(work_dir: &Path) -> PathBuf {
    work_dir.join("merge-resolver-attempts")
}

/// The counter file for `stage_id` under the `.loom/work` directory `work_dir`.
pub(super) fn attempts_file(work_dir: &Path, stage_id: &str) -> PathBuf {
    attempts_dir(work_dir).join(format!("{stage_id}.count"))
}

/// Resolver sessions spawned so far for `stage_id`; 0 when none is recorded.
/// Reads only, so status collection may call it.
pub(crate) fn merge_resolver_attempts(work_dir: &Path, stage_id: &str) -> u32 {
    std::fs::read_to_string(attempts_file(work_dir, stage_id))
        .ok()
        .and_then(|count| count.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{attempts_dir, attempts_file, merge_resolver_attempts};

    fn work_dir_with_count(count: &str) -> tempfile::TempDir {
        let work_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(attempts_dir(work_dir.path())).unwrap();
        std::fs::write(attempts_file(work_dir.path(), "s"), count).unwrap();
        work_dir
    }

    #[test]
    fn max_merge_resolver_attempts_matches_default_retries() {
        // The merge-resolver respawn cap should mirror the crash-retry cap so
        // both failure-bounding mechanisms agree on "3 attempts".
        assert_eq!(super::MAX_MERGE_RESOLVER_ATTEMPTS, 3);
    }

    #[test]
    fn a_missing_counter_reads_as_zero() {
        let work_dir = tempfile::tempdir().unwrap();
        assert_eq!(merge_resolver_attempts(work_dir.path(), "s"), 0);
    }

    #[test]
    fn a_recorded_counter_reads_back() {
        let work_dir = work_dir_with_count("2\n");
        assert_eq!(merge_resolver_attempts(work_dir.path(), "s"), 2);
    }

    #[test]
    fn an_unparseable_counter_reads_as_zero() {
        let work_dir = work_dir_with_count("garbage");
        assert_eq!(merge_resolver_attempts(work_dir.path(), "s"), 0);
    }
}
