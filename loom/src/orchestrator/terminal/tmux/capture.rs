//! Reading the visible text of a session's tmux pane.
//!
//! This is a read of text for a report (what a parked stage's pane showed),
//! never a liveness signal: the monitor derives life and death from verified
//! process identity only (see `TmuxBackend::is_session_alive`) and must never
//! consult this module for it.

use crate::models::session::Session;

use super::viewer::tmux_session_name;
use super::{run_tmux_control, socket_name, socket_path_for, TMUX_PROBE_TIMEOUT};

/// Pure builder for the `tmux capture-pane` argv (excluding the `tmux`
/// binary): joined wrapped lines (`-J`), the last `lines` rows of scrollback
/// plus the visible screen (`-S -<lines>`). Arguments are passed as argv
/// elements, so nothing is shell-quoted.
pub(super) fn capture_pane_argv(socket: &str, session_name: &str, lines: usize) -> Vec<String> {
    [
        "-L",
        socket,
        "capture-pane",
        "-p",
        "-J",
        "-t",
        session_name,
        "-S",
    ]
    .iter()
    .map(|part| (*part).to_string())
    .chain(std::iter::once(format!("-{lines}")))
    .collect()
}

/// The raw text of the last `lines` rows of `session`'s own tmux pane, or
/// `None` when the session has no tmux name, its server socket does not exist
/// (a dead or never-created server costs no subprocess), or the capture fails.
///
/// Bounded by `TMUX_PROBE_TIMEOUT` because the orchestrator's single poll
/// thread calls it. The text is returned as captured; callers normalise it.
pub(crate) fn capture_pane_tail(session: &Session, lines: usize) -> Option<String> {
    let session_name = tmux_session_name(session)?;
    let socket = socket_name(session);
    if !socket_path_for(&socket).exists() {
        return None;
    }
    let args = capture_pane_argv(&socket, &session_name, lines);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_tmux_control(
        &args,
        TMUX_PROBE_TIMEOUT,
        format!("tmux capture-pane ({socket})"),
    )
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
