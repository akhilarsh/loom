//! Tests for the stall park: a stage that used its automatic recoveries, or
//! whose session never started work, is taken down and left in
//! `NeedsHumanReview` with a reason an operator can act on.
//!
//! The login and pane probes are injected, so no test runs the real `claude`
//! or tmux. The kill is observed through a real process, as in
//! `recover_hung_tests`.

use std::cell::Cell;
use std::path::Path;

use super::recover_hung::StallProbes;
use super::recover_hung_tests::{
    report, stalled_stage, BUDGET_SECS, ESCALATING_SILENCE_SECS as ESCALATING,
};
use super::tests::{handoff_work_dir, orchestrator_for};
use super::*;
use crate::claude::auth::AuthProbe;
use crate::fs::session_files::load_session_exact;
use crate::models::session::{Session, SessionExitReason, SessionStatus};
use crate::orchestrator::monitor::heartbeat::{write_heartbeat, Heartbeat};
use crate::verify::transitions::{load_stage, update_stage};

/// A stage executing behind a live agent, tracked by the daemon.
fn tracked_stall(work: &Path, repo_root: &Path) -> (Session, u32, Orchestrator) {
    let (session, agent_pid) = stalled_stage(work);
    let mut orchestrator = orchestrator_for(work, repo_root);
    orchestrator.graph.mark_executing("test-stage").unwrap();
    orchestrator
        .active_sessions
        .insert("test-stage".to_string(), session.clone());
    (session, agent_pid, orchestrator)
}

/// The only heartbeat a session that never reached a tool call has written.
fn write_session_start(work: &Path, session: &Session) {
    let heartbeat = Heartbeat::new("test-stage".to_string(), session.id.clone())
        .with_activity("Session started".into());
    write_heartbeat(work, &heartbeat).unwrap();
}

/// Whether the stand-in agent outlived `outcome`. A survivor is terminated
/// before anything is asserted, so a failing test leaves no process behind.
fn agent_survived(agent_pid: u32, outcome: anyhow::Result<()>) -> bool {
    let survived = crate::process::is_process_alive(agent_pid);
    if survived {
        let _ = crate::process::terminate(agent_pid);
    }
    outcome.unwrap();
    survived
}

/// Reports `session` silent for `silence_secs` and runs the park with the
/// given login and pane probes.
fn park(
    orchestrator: &mut Orchestrator,
    session: &Session,
    silence_secs: u64,
    login: &dyn Fn() -> Option<AuthProbe>,
    tail: &dyn Fn(&Session) -> Option<String>,
) -> anyhow::Result<()> {
    orchestrator.on_session_hung_with(
        report(&session.id, silence_secs),
        &StallProbes { login, tail },
    )
}

fn review_reason(work: &Path) -> String {
    load_stage("test-stage", work)
        .unwrap()
        .review_reason
        .expect("a parked stage carries its review reason")
}

/// A tracked stall whose stage has already used two automatic recoveries.
fn exhausted_stall(work: &Path, repo_root: &Path) -> (Session, u32, Orchestrator) {
    let stall = tracked_stall(work, repo_root);
    update_stage("test-stage", work, |stage| {
        stage.stall_recoveries = 2;
        Ok(())
    })
    .unwrap();
    stall
}

#[test]
fn exhausted_stall_parks_the_stage_and_takes_the_agent_down() {
    let temp = handoff_work_dir();
    let work = temp.path().join(".loom").join("work");
    let (session, agent_pid, mut orchestrator) = exhausted_stall(&work, temp.path());
    let login_calls = Cell::new(0);
    let login = || -> Option<AuthProbe> {
        login_calls.set(login_calls.get() + 1);
        None
    };
    let tail = |_: &Session| Some("starting\nlogin required".to_string());

    let outcome = park(&mut orchestrator, &session, ESCALATING, &login, &tail);

    assert!(
        !agent_survived(agent_pid, outcome),
        "a parked stage must not keep a live agent in its worktree"
    );
    assert_eq!(
        login_calls.get(),
        0,
        "an exhausted stall needs no login probe"
    );
    let stage = load_stage("test-stage", &work).unwrap();
    assert_eq!(stage.status, StageStatus::NeedsHumanReview);
    assert_eq!(stage.stall_recoveries, 2, "a park is not a recovery");
    assert_eq!(stage.session, None);
    assert!(orchestrator.active_sessions.is_empty());
    let persisted = load_session_exact(&work, &session.id).unwrap().unwrap();
    assert_eq!(persisted.status, SessionStatus::ContextExhausted);
    assert_eq!(persisted.exit_reason, Some(SessionExitReason::Stalled));
    assert!(
        work.join("handoffs")
            .join("test-stage-handoff-001.md")
            .exists(),
        "whoever takes the stage on needs the stalled agent's state"
    );
    let reason = review_reason(&work);
    let expected = format!(
        "stalled: session {} silent 900s (budget 300s, last: Bash) after 2 automatic \
         recoveries; pane: \"login required\"",
        session.id
    );
    assert!(reason.starts_with(&expected), "review reason: {reason}");
    assert!(
        reason.contains("Last pane lines:\nstarting"),
        "review reason: {reason}"
    );
}

#[test]
fn never_worked_session_parks_without_a_recovery_charge() {
    let temp = handoff_work_dir();
    let work = temp.path().join(".loom").join("work");
    let (session, agent_pid, mut orchestrator) = tracked_stall(&work, temp.path());
    write_session_start(&work, &session);
    let login = || {
        Some(AuthProbe::LoggedIn {
            method: "claude.ai".into(),
        })
    };
    let tail = |_: &Session| Some("Welcome to Claude Code".to_string());

    let outcome = park(&mut orchestrator, &session, BUDGET_SECS + 10, &login, &tail);

    assert!(
        !agent_survived(agent_pid, outcome),
        "a session that never worked is taken down at its first report"
    );
    let stage = load_stage("test-stage", &work).unwrap();
    assert_eq!(stage.status, StageStatus::NeedsHumanReview);
    assert_eq!(stage.stall_recoveries, 0, "nothing was done to recover");
    assert!(!graph_has_ready_stage(&orchestrator.graph, "test-stage"));
    assert!(
        !work
            .join("handoffs")
            .join("test-stage-handoff-001.md")
            .exists(),
        "a session that never worked has no state to hand off"
    );
    let reason = review_reason(&work);
    let expected = format!(
        "session {} never started work (no tool activity 310s after start, budget 300s)",
        session.id
    );
    assert!(reason.starts_with(&expected), "review reason: {reason}");
}

#[test]
fn not_logged_in_session_parks_with_the_login_remedy() {
    let temp = handoff_work_dir();
    let work = temp.path().join(".loom").join("work");
    let (session, agent_pid, mut orchestrator) = tracked_stall(&work, temp.path());
    write_session_start(&work, &session);
    let login_calls = Cell::new(0);
    let login = || {
        login_calls.set(login_calls.get() + 1);
        Some(AuthProbe::NotLoggedIn)
    };
    let tail = |_: &Session| Some("Invalid API key · Please run /login".to_string());

    let outcome = park(&mut orchestrator, &session, BUDGET_SECS + 10, &login, &tail);

    assert!(
        !agent_survived(agent_pid, outcome),
        "a session that is not logged in is taken down at its first report"
    );
    assert_eq!(login_calls.get(), 1, "the login is probed once per park");
    let stage = load_stage("test-stage", &work).unwrap();
    assert_eq!(stage.status, StageStatus::NeedsHumanReview);
    assert_eq!(stage.stall_recoveries, 0);
    let reason = review_reason(&work);
    assert_eq!(
        reason.lines().next(),
        Some(
            format!(
                "session {} is not logged in to claude in the stage environment; run claude \
                 /login (as the operator) and then loom stage human-review test-stage --approve",
                session.id
            )
            .as_str()
        )
    );
}
