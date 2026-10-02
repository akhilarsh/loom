//! Fixtures shared by the tests that need a stage with a live session.

use std::path::Path;

use crate::fs::session_files::save_session;
use crate::fs::work_dir::write_terminal_config;
use crate::models::session::{
    Session, SessionBackendKind, SessionStatus, SessionType, TerminalConfig,
};
use crate::orchestrator::terminal::native::write_test_pid_identity;

/// Save a running `session_type` session for `stage_id` whose recorded process
/// is this test process, so liveness probes answer alive.
///
/// The terminal lane is set to tmux: it defers native terminal detection, which
/// fails on a headless runner, to a PID-only probe that reads the identity
/// file written here.
pub(crate) fn write_live_session(
    work_dir: &Path,
    stage_id: &str,
    session_type: SessionType,
) -> Session {
    write_terminal_config(
        work_dir,
        &TerminalConfig {
            backend: SessionBackendKind::Tmux,
        },
    )
    .unwrap();
    let mut session = Session::new();
    session.assign_to_stage(stage_id.to_string());
    session.status = SessionStatus::Running;
    session.session_type = session_type;
    write_test_pid_identity(work_dir, &session, std::process::id()).unwrap();
    save_session(&session, work_dir).unwrap();
    session
}

/// Mark `session` finished on disk, so it no longer counts as live.
pub(crate) fn finish_session(work_dir: &Path, session: &mut Session) {
    session.status = SessionStatus::Completed;
    save_session(session, work_dir).unwrap();
}
