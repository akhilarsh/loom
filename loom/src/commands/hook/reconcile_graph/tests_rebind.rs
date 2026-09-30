//! Tests for [`super::rebind_claim`]: the spawning hook hands its lease to the
//! child it spawned. Pure over a tempdir lock file; the "child" is this
//! process, the only pid a test can rely on being alive.

use super::*;
use tempfile::TempDir;

const DEBOUNCE_SECS: u64 = 600;
const STALE_LOCK_SECS: u64 = 1800;
/// Above `i32::MAX`, so `is_process_alive` reports it dead without a process.
const DEAD_PID: u32 = u32::MAX;

fn seed(lock_path: &Path, epoch: u64, pid: u32) {
    let state = LockState {
        epoch,
        pid,
        failures: 0,
        pending: false,
    };
    update_lock(lock_path, |_| (Some(state), ())).expect("seed the lock");
}

fn second_hook(lock_path: &Path, now: u64) -> LockDecision {
    decide(
        lock_path,
        now,
        DEBOUNCE_SECS,
        STALE_LOCK_SECS,
        crate::process::is_process_alive,
    )
}

#[test]
fn a_rebound_claim_names_the_child_and_blocks_a_second_hook() {
    let temp = TempDir::new().unwrap();
    let lock_path = temp.path().join("reconcile.lock");
    let now = 1_000_000;
    let child = std::process::id();
    seed(&lock_path, now, DEAD_PID);
    assert_eq!(second_hook(&lock_path, now), LockDecision::Spawn);

    rebind_claim(&lock_path, now, DEAD_PID, child);

    assert_eq!(read_lock(&lock_path).unwrap().pid, child);
    assert_eq!(second_hook(&lock_path, now), LockDecision::Skip);
}

#[test]
fn a_rebind_does_not_touch_a_lease_that_moved_on() {
    let temp = TempDir::new().unwrap();
    let lock_path = temp.path().join("reconcile.lock");
    let now = 1_000_000;
    let child = std::process::id();

    seed(&lock_path, now + 5, 4242);
    rebind_claim(&lock_path, now, 4242, child);
    assert_eq!(read_lock(&lock_path).unwrap().pid, 4242, "other epoch");

    seed(&lock_path, now, 4243);
    rebind_claim(&lock_path, now, 4242, child);
    assert_eq!(read_lock(&lock_path).unwrap().pid, 4243, "other holder");
}
