# S1: a stage agent's `loom stage block` retires its session, and its exit is not a crash

Stage `stage-exits`, tier opus. Read `../common.md` first.

You own `loom/src/orchestrator/core/event_handler.rs`,
`loom/src/orchestrator/core/event_handler/{stage_takedown.rs, blocked.rs (new),
verdict_retirement_tests.rs}` and `loom/src/orchestrator/monitor/{session_events.rs,
session_events/tests.rs}`.

## The defect (verified)

`loom stage block <id> "<reason>"` from a stage agent reaches
`daemon/server/control_block.rs::handle_block_stage` (relayed, spooled or over the socket),
which marks the stage Blocked and sets `close_reason`; no `failure_info`. The monitor sees the
status change (`monitor/detection.rs:66-89`) and emits `StageBlocked`, which
`event_handler.rs::handle_one_event` (about lines 173-177) handles by printing a line and
marking the graph. Nothing touches the session: the agent stays alive and idle. When its
process later exits, `session_events.rs::detect_vanished_process` (about lines 204-218) runs
`exited_after_stage_finished` (about lines 308-323), which forgives only `Completed`,
`MergeConflict` and `MergeBlocked`, so the exit falls to `record_crash`, and
`core/crash_handler.rs::persist_blocked_crash` (about lines 248-272) sets `failure_info`,
increments `retry_count` and overwrites `close_reason`. A crash is auto-retried
(`orchestrator/retry.rs:12-21`), so the stage re-runs into the same wall and the reason is
lost. If the agent is still alive when the operator runs `loom stage retry`, retry refuses
without `--force` (`commands/stage/skip_retry.rs:54-69`), and the executor adopts a live
session instead of spawning.

## 1. Retire on `StageBlocked`

`event_handler/stage_takedown.rs::retire_disputing_agents` (about lines 296-332) is the
takedown to reuse: it writes a `HandoffOrigin::Retired` handoff per agent through
`self.monitor.handlers().ensure_context_handoff`, kills them with `take_down_agents` (which
confirms each is gone and records it `ContextExhausted` with the given exit reason), and clears
`stage.session` under `update_stage` only while the stage is still in the expected status. It
returns early unless the stage is `NeedsAdjudication`. Generalize without changing its callers
(`core/verdict_apply.rs:27`, `core/orchestrator.rs:285`):

- `pub(super) fn retire_stage_agents(&mut self, stage_id: &str, expected: StageStatus) ->
  Result<Vec<String>>` holds today's body with `expected` in place of `NeedsAdjudication`
  (both the early return and the guard before `release_session`).
- `retire_disputing_agents` becomes a thin wrapper passing `StageStatus::NeedsAdjudication`;
  keep its doc comment.
- Rename `disputing_agents` to `retirable_agents` and make its doc and its warning text
  neutral (it serves both callers; it still filters out the adjudication session).
- Update the doc comment at about lines 136-145 that says `exited_after_stage_finished`
  "forgives only Completed/MergeConflict/MergeBlocked".
- Keep `stage_takedown.rs` under 400 lines (it is 376).

New `event_handler/blocked.rs`, declared `mod blocked;` beside `mod stage_takedown;` in
`event_handler.rs`:

    /// A stage that became Blocked. Blocked with no `failure_info` means `loom stage block`
    /// (a stage agent's, an operator's) or a rejected human review, never a crash: retire any
    /// live agent so its later exit is not filed as a crash and a retry spawns a fresh session.
    pub(super) fn on_stage_blocked(&mut self, stage_id: &str) -> Result<()>

1. `self.graph.mark_status(stage_id, StageStatus::Blocked)?` (what the arm does today).
2. Load the stage. Return when it is no longer Blocked (a retry moved it on) or it has
   `failure_info` (a crash or an infrastructure block: its session is already gone or the crash
   path owns it).
3. `retire_stage_agents(stage_id, StageStatus::Blocked)`; for survivors print a warning naming
   them (the stage stays Blocked; the operator can `loom stage reset <id> --kill-session`).
   `close_reason` is never written here.

The ordering that commit 0499aa3d set for escalations applies in its Blocked form: the status
is already written when the daemon learns of the block, so retirement runs while the stage is
Blocked and clears `stage.session` only if it still is. A stage retried in between keeps its
new session.

`event_handler.rs::handle_one_event` is ledgered at exactly 116 lines: in the
`MonitorEvent::StageBlocked` arm replace the line
`self.graph.mark_status(&stage_id, StageStatus::Blocked)?;` with
`self.on_stage_blocked(&stage_id)?;` and change nothing else in the function. If that leaves an
import unused, remove it from the `use` line without changing the file's other lines more than
needed.

## 2. The exit is not a crash

`session_events.rs::exited_after_stage_finished`: the stage also counts as finished when it is
`Blocked` with `failure_info` `None`. Update the doc comment. This covers an agent that exits
on its own before the daemon retires it. A Contract session reaches `ended_contract_session`
first; on a Blocked stage `on_contract_session_ended` is a no-op
(`event_handler/contract_phase.rs::current_contract_session` requires `Executing`), so leave
that order alone.

## 3. Tests (exact names; acceptance runs two of them)

In `session_events/tests.rs`, mirroring
`monitor/tests/merge_sessions.rs::test_merge_conflict_stage_session_not_reported_as_crash`
(a `Handlers` with `LivenessService::fixed_for_tests(false)`, two polls of
`Detection::detect_session_changes`):

- `an_agent_blocked_stage_session_exit_is_not_a_crash`: Blocked, `failure_info` None,
  `close_reason` Some; no `SessionCrashed` event and the session's last state is `Completed`.
- `a_crash_blocked_stage_session_exit_is_still_a_crash`: Blocked with `failure_info` Some; the
  exit is reported as before.

In `event_handler/verdict_retirement_tests.rs`, mirroring
`retiring_kills_the_disputing_agent_and_clears_the_stage_session` (a real stand-in process from
`spawn_orphan_process`, `write_test_pid_identity`, `assign_stage_session`):

- `an_agent_block_retires_the_live_session`: an Executing stage with a live agent is blocked
  the way `handle_block_stage` does it (`try_mark_blocked`, `close_reason = Some(..)`); after
  `on_stage_blocked("test-stage")` the process is gone, `stage.session` is `None`, the status is
  still Blocked, `close_reason` is unchanged, `failure_info` is `None`, and the session record is
  terminal.
- `a_crash_blocked_stage_is_left_to_the_crash_path`: with `failure_info` set, the agent is not
  killed.
- `a_stage_that_left_blocked_is_not_retired`: status moved on to Queued before the handler
  runs; nothing is killed.

## Check

`cargo test --lib orchestrator::core::event_handler::verdict_retirement_tests`, once.

## Not yours

`loom status` showing the reason is S2's; the doctrine is S3's.
