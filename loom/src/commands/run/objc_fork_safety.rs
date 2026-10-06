//! macOS daemonization guard.
//!
//! `DaemonServer::start` daemonizes with `fork()`. The Objective-C runtime
//! aborts a forked child that touches a class whose `+initialize` was in
//! progress on another thread at fork time ("Crashing instead"), so the
//! daemon died right after `loom run` returned. The runtime reads
//! `OBJC_DISABLE_INITIALIZE_FORK_SAFETY` only at process start, so setting it
//! in-process is too late: `loom run` re-executes itself once with it set.

#[cfg(target_os = "macos")]
const OBJC_FORK_SAFETY_VAR: &str = "OBJC_DISABLE_INITIALIZE_FORK_SAFETY";

#[cfg(target_os = "macos")]
fn needs_reexec(current: Option<&str>) -> bool {
    current != Some("YES")
}

/// Re-executes the current process with the Objective-C fork-safety abort
/// disabled. Returns only when no re-exec is needed or the exec failed, in
/// which case `loom run` continues and the daemon start reports its own failure.
pub(super) fn ensure_fork_safe_environment() {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;

        if !needs_reexec(std::env::var(OBJC_FORK_SAFETY_VAR).ok().as_deref()) {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let error = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(OBJC_FORK_SAFETY_VAR, "YES")
            .exec();
        tracing::warn!("could not re-execute with {OBJC_FORK_SAFETY_VAR} set: {error}");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::needs_reexec;

    #[test]
    fn reexec_is_needed_until_the_variable_is_yes() {
        assert!(needs_reexec(None));
        assert!(needs_reexec(Some("NO")));
        assert!(!needs_reexec(Some("YES")));
    }
}
