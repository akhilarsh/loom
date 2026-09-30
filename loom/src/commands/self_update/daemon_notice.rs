//! The warning `loom update` prints when a daemon started before the update
//! is still running the previous binary.

use std::path::Path;

use crate::daemon::DaemonServer;
use crate::process::{verify_process_identity, IdentityStatus};

/// The warning naming a live daemon of `work_dir` that predates the update;
/// `None` when the state directory records no daemon or its process is gone.
///
/// `work_dir` is canonicalized first (falling back to itself): in a stage
/// worktree it is the `.loom/work` symlink, which the no-follow read of the
/// pid file would refuse.
pub(super) fn stale_daemon_warning(work_dir: &Path) -> Option<String> {
    let root = work_dir
        .canonicalize()
        .unwrap_or_else(|_| work_dir.to_path_buf());
    let identity = DaemonServer::read_identity(&root)?;
    if verify_process_identity(identity) == IdentityStatus::Dead {
        return None;
    }
    Some(format!(
        "the running daemon (pid {}) still runs the previous binary; restart it with `loom stop && loom run`",
        identity.pid
    ))
}

/// Print [`stale_daemon_warning`] to stderr for the workspace containing the
/// current directory. No workspace, or no live daemon, prints nothing.
pub(super) fn warn_if_daemon_is_stale() {
    let Ok(work_dir) = crate::commands::common::resolve_work_dir() else {
        return;
    };
    if let Some(warning) = stale_daemon_warning(work_dir.root()) {
        eprintln!("warning: {warning}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::process_start_time;

    fn write_pid_file(work_dir: &Path, content: &str) {
        std::fs::write(work_dir.join("orchestrator.pid"), content).unwrap();
    }

    #[test]
    fn a_live_daemon_pid_warns_with_the_pid_and_restart_command() {
        let work_dir = tempfile::tempdir().unwrap();
        let pid = std::process::id();
        let start = process_start_time(pid).map_or("-".to_string(), |time| time.to_string());
        write_pid_file(work_dir.path(), &format!("{pid} {start}\n"));

        let warning = stale_daemon_warning(work_dir.path()).expect("a live pid must warn");
        assert!(warning.contains(&format!("pid {pid}")), "{warning}");
        assert!(warning.contains("loom stop && loom run"), "{warning}");
    }

    #[test]
    fn a_symlinked_work_dir_still_warns_for_a_live_daemon() {
        let real = tempfile::tempdir().unwrap();
        let pid = std::process::id();
        let start = process_start_time(pid).map_or("-".to_string(), |time| time.to_string());
        write_pid_file(real.path(), &format!("{pid} {start}\n"));
        let links = tempfile::tempdir().unwrap();
        let link = links.path().join("work");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();

        let warning = stale_daemon_warning(&link).expect("a symlinked work dir must warn");
        assert!(warning.contains(&format!("pid {pid}")), "{warning}");
    }

    #[test]
    fn a_dead_pid_does_not_warn() {
        let work_dir = tempfile::tempdir().unwrap();
        write_pid_file(work_dir.path(), "999999999 -\n");
        assert_eq!(stale_daemon_warning(work_dir.path()), None);
    }

    #[test]
    fn no_pid_file_does_not_warn() {
        let work_dir = tempfile::tempdir().unwrap();
        assert_eq!(stale_daemon_warning(work_dir.path()), None);
    }
}
