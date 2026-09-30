//! `loom hook reconcile-graph --cancel`: stop the lease holder, but only when
//! the pid the lock names is still the reconciler that recorded it.
//!
//! A pid alone is not an identity: the holder may have exited and the pid been
//! reused by an unrelated process (the daemon, `loom stage complete`). The
//! holder is signalled only when its argv holds both `hook` and
//! `reconcile-graph` AND it started no later than the lock's epoch (the
//! reconciler stamps the epoch after it starts). Anything else is left alone.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use super::lock::{read_lock, release_lease};

/// Whole seconds of slack between a process's start (`ps` reports elapsed time
/// in whole seconds) and the epoch its reconciler stamped.
const START_SKEW_SECS: u64 = 2;

/// Stop the live holder named by `lock_path`, then record its run as finished
/// with `failures` unchanged. With no holder recorded, or a recorded holder
/// that is already dead, this does nothing: the line stays an in-progress
/// marker with a dead pid, which the next hook takes over at once instead of
/// waiting out a debounce a fresh release would start.
pub(super) fn cancel_holder(lock_path: &Path, now: u64) {
    let Some(holder) = read_lock(lock_path)
        .filter(|state| state.pid != 0 && crate::process::is_process_alive(state.pid))
    else {
        return;
    };
    if is_reconciler(holder.pid, holder.epoch, now) {
        if let Err(error) = crate::process::terminate(holder.pid) {
            tracing::debug!(%error, pid = holder.pid, "reconcile-graph: could not signal the holder");
        }
    }
    release_lease(lock_path, now, holder.pid);
}

/// Whether `pid` is a `loom hook reconcile-graph` process that started no
/// later than `epoch`. Unknown facts (no such process, no `ps`) count as no.
fn is_reconciler(pid: u32, epoch: u64, now: u64) -> bool {
    let Some(argv) = process_argv(pid) else {
        return false;
    };
    let names = |word: &str| argv.iter().any(|arg| arg == word);
    if !(names("hook") && names("reconcile-graph")) {
        return false;
    }
    let Some(elapsed) = ps_field(pid, "etime=").as_deref().and_then(parse_etime) else {
        return false;
    };
    now.saturating_sub(elapsed) <= epoch.saturating_add(START_SKEW_SECS)
}

/// The process's argv: NUL-split from `/proc/<pid>/cmdline`, else the
/// whitespace-split `ps -o command=` line.
fn process_argv(pid: u32) -> Option<Vec<String>> {
    if let Ok(raw) = fs::read(format!("/proc/{pid}/cmdline")) {
        return Some(
            raw.split(|byte| *byte == 0)
                .filter(|arg| !arg.is_empty())
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
                .collect(),
        );
    }
    let line = ps_field(pid, "command=")?;
    Some(line.split_whitespace().map(str::to_string).collect())
}

/// One `ps -p <pid> -o <field>` value, trimmed; `None` when `ps` is missing,
/// fails, or prints nothing (no such process).
fn ps_field(pid: u32, field: &str) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", field])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// Parse `ps`'s `etime` (`[[dd-]hh:]mm:ss`) into seconds.
fn parse_etime(text: &str) -> Option<u64> {
    let (days, clock) = match text.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, text),
    };
    let fields: Vec<&str> = clock.split(':').collect();
    if fields.len() > 3 {
        return None;
    }
    let mut seconds = 0u64;
    for field in fields {
        seconds = seconds.checked_mul(60)?.checked_add(field.parse().ok()?)?;
    }
    days.checked_mul(86_400)?.checked_add(seconds)
}

#[cfg(test)]
mod tests {
    use super::parse_etime;

    #[test]
    fn etime_parses_every_ps_shape() {
        assert_eq!(parse_etime("00:07"), Some(7));
        assert_eq!(parse_etime("12:34"), Some(754));
        assert_eq!(parse_etime("01:02:03"), Some(3723));
        assert_eq!(parse_etime("2-01:00:00"), Some(2 * 86_400 + 3_600));
    }

    #[test]
    fn etime_rejects_malformed_text() {
        assert_eq!(parse_etime(""), None);
        assert_eq!(parse_etime("a:b"), None);
        assert_eq!(parse_etime("1:2:3:4"), None);
    }
}
