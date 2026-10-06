# E2: stall parking and never-worked fail-fast

Tier: opus. Read `doc/plans/briefs/open-issues-19-24/common.md` first; this brief adds only E2's part.
Plan Decision 12 (#20 A and B). Line numbers read at `ff3fe947`; `platform-portability` and E1/E3 land
around you, so anchor on symbols.

## Files owned (write only these; all under `loom/`)

`src/orchestrator/core/event_handler/recover_hung.rs`, `.../recover_hung_tests.rs`,
`.../recover_hung_park_tests.rs` (new), `src/orchestrator/core/event_handler.rs`,
`src/orchestrator/core/loop_recovery/mod.rs`, `.../loop_recovery/park.rs` (you will not need it),
`src/orchestrator/core/heartbeat_apply.rs`, `src/orchestrator/monitor/hung_latch.rs`,
`src/orchestrator/monitor/never_worked.rs` (new), `src/orchestrator/monitor/events.rs`,
`src/orchestrator/monitor/mod.rs`, `src/orchestrator/monitor/tests/mod.rs`,
`src/orchestrator/monitor/tests/never_worked.rs` (new).

Read first: `recover_hung.rs` (whole, 221 lines), `hung_latch.rs:54-159` and `:283-313`,
`monitor/heartbeat.rs:49-96` (`Heartbeat`), `:310-334` (`check_session_hung`), `loop_recovery/mod.rs:64-99`
(`finish_handoff_and_requeue`), `loop_recovery/park.rs:213-231` (`set_completion_review`),
`event_handler.rs:316-332` (`begin_handoff`), `event_handler/stage_takedown.rs:190` (`take_down_stage_agents`),
`monitor/tests/heartbeats.rs:13-37` (fixtures).

## Pinned interfaces

You CONSUME (quoted from common.md):

- E1: "`pub fn stage_auth_status(claude_path: &Path) -> AuthProbe`" and "`#[derive(Debug, Clone, PartialEq, Eq)]
  pub enum AuthProbe { LoggedIn { method: String }, NotLoggedIn, Unknown(String) }`" in `crate::claude::auth`;
  the binary is found with `crate::claude::find_claude_path()`.
- E3: "`pub fn session_tail(session: &crate::models::session::Session, work_dir: &Path, lines: usize) ->
  Option<String>`: the last `lines` non-empty lines of the session's tmux pane ... or for a native session the
  tail of its stderr log; `None` when nothing is readable", at `crate::orchestrator::terminal::session_tail`.

You PROVIDE the park reasons, exactly (common.md "Park reasons (E2)"): "Exhaustion: `stalled: session <id>
silent <N>s (budget <B>s, last: <activity>) after <k> automatic recoveries; pane: "<last line>"`. Never worked,
logged in: `session <id> never started work (no tool activity <N>s after start, budget <B>s); pane: "<last
line>"`. Not logged in: `session <id> is not logged in to claude in the stage environment; run claude /login
(as the operator) and then loom stage human-review <stage> --approve`. Pane tail lines go to `review_notes`."

## Root cause (re-verified at ff3fe947)

1. `recover_hung.rs:56-72` `stall_recovery_exhausted` only `eprintln!`s ("Leaving it exactly where it is") and
   returns, so the stage stays `Executing` with its agent alive. The escalated latch
   (`hung_latch.rs:220-244` `hung_report_due`/`record_hung_report`) then suppresses every later report. Nothing
   resets `stall_recoveries` (E3's part).
2. `hung_latch.rs:139` `HeartbeatStatus::NoHeartbeat => return None`: a session that never wrote any heartbeat is
   never reported. A session whose only heartbeat is SessionStart's (`loom-hooks/session-start.sh:115-119`:
   `last_tool: null`, `activity: "Session started"`) is reported at 1x but only recovered at 3x
   (`STALL_ESCALATION_MULTIPLIER`, `hung_latch.rs:45`), and recovery re-queues it, charging the budget.
3. A parked state exists: `NeedsHumanReview` with `review_reason` announces itself. The detection loop
   (`detection.rs` `detect_stage_changes`) emits `StageNeedsHumanReview` on the transition, `event_handler/human_review.rs`
   `announce_needs_human_review` prints it and calls `orchestrator/notify.rs` `notify_needs_human_review`. So
   NO direct notify call is written here.
4. `Stage` has no `review_notes` field. `StageSummary.review_notes` is built from `stage.review_reason`
   (`commands/status/data/collector.rs:244`, multi-line kept; `review_reason` is flattened to 200 chars in the
   summary, `data/sanitize.rs:54-56`). So "pane tail goes to review_notes" means: the stored `review_reason` is
   the one-line reason, then a blank line, `Last pane lines:`, then the tail.

## Design (settled)

Do NOT add a field to `MonitorEvent::SessionHung` or `HungReport`: exact struct literals of both live in tests
no worker owns (`monitor/completion_blockers_tests.rs:255`, `monitor/tests/heartbeats.rs:101`) and in
`recover_hung_tests.rs`. Instead the handler decides "never worked" itself, from the same pure predicate the
detector uses.

- `monitor/never_worked.rs` (new, `pub(crate) mod never_worked;` in `monitor/mod.rs` after `mod input_wait;`):

```rust
pub(crate) fn silence_without_heartbeat(session: &Session, now: DateTime<Utc>, budget_secs: u64) -> Option<u64>;
pub(crate) fn never_worked(session: &Session, heartbeat: Option<&Heartbeat>, now: DateTime<Utc>, budget_secs: u64) -> bool;
pub(crate) fn is_escalation(stale_duration_secs: u64, timeout_secs: u64, never_worked: bool) -> bool;
```

- `silence_without_heartbeat`: `None` when `budget_secs == 0`, `session.session_type != SessionType::Stage`,
    `session.context_tokens > 0`, or `now - session.created_at < budget_secs`; else `Some(seconds since created_at)`.
- `never_worked`: false when `budget_secs == 0`, the session is not `SessionType::Stage`, or
    `session.context_tokens > 0`. Otherwise, with `heartbeat.filter(|hb| hb.session_id == session.id)`: `Some(hb)`
    gives `hb.last_tool.is_none() && !hb.subagent`; `None` gives `silence_without_heartbeat(..).is_some()`.
    The `context_tokens > 0` guard is mandatory: `session-start.sh` also fires on compact and resume
    and rewrites the heartbeat with `last_tool: null`, so a working session that just compacted has the
    SessionStart shape; its record already carries tokens from earlier tool heartbeats
    (`core/heartbeat_apply.rs` persists them), a never-worked session's stays 0.
- `is_escalation(stale, timeout, true)` is `timeout > 0 && stale >= timeout`; with `false` it is
    `hung_latch::is_stall_escalation(stale, timeout)` (3x, unchanged).
- Detection (`hung_latch.rs`, `stage_silence_event`): replace the `HeartbeatStatus::NoHeartbeat => return None` arm
  with `HeartbeatStatus::NoHeartbeat => silence_without_heartbeat(session, heartbeat_watcher.now(), timeout_secs)?`
  so such a session yields a normal `SessionHung` (alive check, latch and `hung_event` unchanged). In
  `hung_event`, take `last_activity` only from a heartbeat whose `session_id == session.id`
  (`.get_heartbeat(stage_id).filter(|hb| hb.session_id == session.id).and_then(..)`): a previous session's
  activity text must not describe this one. `hung_latch.rs` is 385 lines: net growth must stay under 15 lines
  (import, the arm, the filter). Do not add tests there; the 1x and zero-budget tests live in
  `never_worked.rs` and `monitor/tests/never_worked.rs`. Update `events.rs`'s `SessionHung` doc comment
  (`:52`) to add "or none at all since spawn".
- Handler (`recover_hung.rs`): `on_session_hung(report)` keeps its signature and calls
  `on_session_hung_with(report, &StallProbes::production(work_dir))`.

```rust
pub(super) struct StallProbes<'a> {
    pub login: &'a dyn Fn() -> Option<AuthProbe>,
    pub tail: &'a dyn Fn(&Session) -> Option<String>,
}
```

  Production `login`: `|| crate::claude::find_claude_path().ok().map(|path| stage_auth_status(&path))` (the literal
  `stage_auth_status(` must appear in `recover_hung.rs`); production `tail`:
  `|s| session_tail(s, &work_dir, 40)`. Tests inject both, so no test runs the real `claude` or tmux.

## Tasks

1. `never_worked.rs` as above, with inline tests (see Tests).
2. `hung_latch.rs`, `events.rs`, `monitor/mod.rs`: as above.
3. `on_session_hung_with(report, probes)`: print the warning (unchanged); no stage gives `Ok`; load the session
   (`load_session_exact(&self.config.work_dir, id)`, an `Err` is logged with `tracing::warn!` and treated as no
   session) and the heartbeat (`read_heartbeat(&heartbeat_path(&work_dir, stage_id)).ok()`); compute
   `never_worked(..., Utc::now(), report.timeout_secs)`; if `!is_escalation(stale, timeout, never_worked)` return
   `Ok`; else `recover_stalled_stage(stage_id, &report, never_worked, session.as_ref(), probes)`.
4. `recover_stalled_stage`: keep the three existing early returns (session/status mismatch, then
   `completion_blocker_owns_stage`). Then:
   - `never_worked`: capture the tail (`probes.tail`) BEFORE any takedown, probe the login once
     (`probes.login`); reason is `not_logged_in_reason` only for `Some(AuthProbe::NotLoggedIn)`, else
     `never_worked_reason` (a `LoggedIn`, `Unknown` or missing claude binary is not a definite answer). Then
     `begin_handoff` (identity check, `None` gives `Ok`), NO stall handoff (nothing was done), then
     `finish_handoff_and_park(stage_id, session_id, review_reason, SessionExitReason::Stalled)`. No
     `charge_stall_recovery`.
   - `stage.stall_recoveries >= MAX_STALL_RECOVERIES`: capture the tail, `begin_handoff`, `write_stall_handoff`,
     then `finish_handoff_and_park(.., exhausted_reason(..), SessionExitReason::Stalled)`. No probe, no charge.
     Delete `stall_recovery_exhausted` and its `eprintln!`; `rg -q -F 'Leaving it exactly where it is' src` must find nothing.
   - otherwise the existing requeue path, untouched.
   Print one `eprintln!` line per park (`SESSION STALLED:` style as the existing requeue branch does).
5. Reason builders (pure `fn`s in `recover_hung.rs`; if the file passes 390 lines move them to
   `loop_recovery/park.rs`): `exhausted_reason`, `never_worked_reason`, `not_logged_in_reason`, plus
   `with_pane_notes(reason, tail) -> String` (appends `"\n\nLast pane lines:\n{tail}"` when the tail is `Some`).
   `<last line>` is the tail's last non-empty line, cut to 120 chars, `"` replaced by `'`; with no tail the
   `; pane: "..."` segment is omitted entirely. `<activity>` is `report.last_activity` or `none`.
6. `loop_recovery/mod.rs`, beside `finish_handoff_and_requeue`:

```rust
pub(super) fn finish_handoff_and_park(&mut self, stage_id: &str, session_id: &str,
    review_reason: String, reason: SessionExitReason) -> Result<()>
```

   `take_down_stage_agents(stage_id, session_id, reason)?`; if survivors remain, print the same
   stays-in-`NeedsHandoff` message as the requeue twin (kill them with `loom stage reset {stage_id}
   --kill-session`) and return `Ok`. Else in one `update_stage` closure: the identity check
   (`event_targets_current_session` and `status == NeedsHandoff`, else `Ok` and no change),
   `stage.force_status_with_reason(StageStatus::NeedsHumanReview, &review_reason)`,
   `stage.review_reason = Some(review_reason)`, `stage.release_session()`; then `self.graph.mark_status(stage_id,
   StageStatus::NeedsHumanReview)?` only when the closure applied. Modelled on `park.rs` `set_completion_review`
   except: no `accumulate_attempt_time` (the earlier `begin_handoff` -> `mark_needs_handoff` already accumulated it
   and took `attempt_started_at`, so a second call is a no-op), and `release_session` is added so
   `refuse_live_worker` in `human_review --approve` sees no session. The agent MUST be taken down on both park
   paths: `handle_approve` refuses while a live worker exists.
7. `event_handler.rs`: register `#[cfg(test)] mod recover_hung_park_tests;` after `mod recover_hung_tests;`
   (`:375`), nothing else (383 lines).
8. `heartbeat_apply.rs:20-25`: replace the stale "Hung detection stays advisory ..." paragraph with: hung
   reports are acted on by `event_handler/recover_hung.rs`; this module only makes the session record true and adds no policy.
9. `recover_hung.rs` module doc (lines 1-23): update items 3 and the closing paragraph to the new truth
   (exhaustion parks the stage; never-worked parks at 1x), no history narrative.

## Tests (exact paths)

- `orchestrator::core::event_handler::recover_hung_park_tests` (new file; exactly these three tests). Reuse
  the fixtures of `recover_hung_tests.rs` by making `report`, `stalled_stage`, `BUDGET_SECS` and
  `ESCALATING_SILENCE_SECS` there `pub(super)` (visibility change only; no assertion line moves). Probe closures
  count calls with a `std::cell::Cell`.
  - `exhausted_stall_parks_the_stage_and_takes_the_agent_down`: `stall_recoveries = 2` via `update_stage`;
    `on_session_hung_with(report(&session.id, ESCALATING_SILENCE_SECS), probes)` with `tail` returning
    `Some("starting\nlogin required")` and a `login` closure that must not be called. Assert: agent pid not
    alive, status `NeedsHumanReview`, `stall_recoveries == 2`, `session == None`, `active_sessions` empty,
    persisted session `ContextExhausted` with `exit_reason == Some(Stalled)`, the stall handoff file exists
    (`handoffs/test-stage-handoff-001.md`), `review_reason` starts with `stalled: session <id> silent 900s (budget
    300s, last: Bash) after 2 automatic recoveries; pane: "login required"` and contains `Last pane lines:\nstarting`.
  - `never_worked_session_parks_without_a_recovery_charge`: write `Heartbeat::new("test-stage", &session.id)
    .with_activity("Session started".into())` with `write_heartbeat(&work, ..)`; report at `BUDGET_SECS + 10`
    (1x, below 3x); `login` returns `Some(AuthProbe::LoggedIn { method: "claude.ai".into() })`. Assert:
    parked, `stall_recoveries == 0`, agent dead, not in `graph_has_ready_stage`, no handoff file, reason starts
    with `session <id> never started work (no tool activity 310s after start, budget 300s)`.
  - `not_logged_in_session_parks_with_the_login_remedy`: same fixture, `login` returns
    `Some(AuthProbe::NotLoggedIn)` and counts calls. Assert the first line of `review_reason` equals
    `session <id> is not logged in to claude in the stage environment; run claude /login (as the operator) and
    then loom stage human-review test-stage --approve` exactly, probe called once, `stall_recoveries == 0`, agent dead.
- `recover_hung_tests.rs`: the old `the_third_stall_leaves_the_stage_for_an_operator` (doc comment from
  `/// The bound.` at `:238` through the closing brace, ~`:272`) pins behaviour this stage removes (agent alive,
  stage `Executing`). Its assertion lines cannot stay true, so DELETE it (and any import only it used) and say
  so in your report: deleting assertions raises a test-integrity event, and the orchestrator will dispute it.
  Edit no other assertion line. The sibling tests (`a_report_below_the_escalation_line_only_warns`, the 900 s
  requeue test, the checkpoint test) stay true: their sessions are fresh (`created_at` now) with no heartbeat
  file, so `never_worked` is false.
- `orchestrator::monitor::never_worked::tests` (inline): truth table for `never_worked` (own SessionStart
  heartbeat gives true; own tool heartbeat gives false; subagent heartbeat gives false; other session's heartbeat
  and age under budget gives false, over budget gives true; `context_tokens > 0` gives false; Merge session gives
  false; zero budget gives false) and `is_escalation` (`(300, 300, true)` true; `(299, 300, true)` false;
  `(86_400, 0, true)` false; `(310, 300, false)` false; `(900, 300, false)` true).
- `orchestrator::monitor::tests::never_worked` (new file; `mod never_worked;` in `monitor/tests/mod.rs`).
  `fixed_harness` and `running_pair` in `tests/heartbeats.rs` are private, so copy their ~20 lines (that file
  is not yours). Tests: `a_session_with_no_heartbeat_past_its_budget_is_reported_hung` (stage budget 60,
  `session.created_at = now - 120 s`; one `SessionHung { stale_duration_secs: 120, timeout_secs: 60,
  last_activity: None, .. }`), `a_session_with_no_heartbeat_inside_its_budget_is_not_reported`,
  `a_previous_sessions_heartbeat_does_not_describe_a_never_started_session` (heartbeat for `old-session` written
  with `write_heartbeat`; event has `last_activity: None`), `a_zero_budget_never_reports_a_session_without_a_heartbeat`,
  `a_merge_session_without_a_heartbeat_is_not_reported`.

## Patterns to copy

`set_completion_review` and `park_completion` (`park.rs:173-231`) for the park write; `finish_handoff_and_requeue`
for the survivor handling; `recover_hung_tests.rs:30-58` fixtures; `judge_last_activity` (`hung_latch.rs:272-281`)
for "measure from `created_at` when no own heartbeat". DO NOT copy `set_completion_review`'s `ensure_current_writer`
(it demands status `Executing`; after `begin_handoff` the stage is `NeedsHandoff`), nor `park_completion`'s
`remove_signal` (not needed here).

## Traps (knowledge, quoted)

- "killing a working agent is worse than waiting for a stuck one" (`recover_hung.rs` header) and
  `mistakes/sessions-and-liveness.md`: park only after confirmed death. "confirmed alive: request/perform the
  existing safe takedown, then park only after confirmed death" (loop-recovery brief): never mark the stage
  parked while `take_down_stage_agents` reports survivors.
- "every external command issued from the poll loop goes through `process::run_bounded`"
  (`mistakes/sessions-and-liveness.md`): `stage_auth_status` (30 s bound) and `session_tail` (5 s) run on the
  single poll thread once per park, never per poll; keep both behind the guards above.
- Tests: never spawn the real `claude` or tmux, never touch the live `.loom/work`, never leave a process alive
  (`mistakes/detached-spawn-in-tests.md`): the stand-in agent from `spawn_orphan_process` is terminated by the
  takedown, and on any early failure the existing tests call `crate::process::terminate(agent_pid)`; do the same.
- `begin_handoff` moves the stage to `NeedsHandoff`; `stage_agents` (used by `take_down_stage_agents`) only looks
  at `NeedsHandoff` stages, so call order is `begin_handoff`, then takedown, then park.
- The pane tail is agent-controlled text. It goes only into the stored `review_reason`, whose renderers already
  sanitize it (`context/untrusted.rs`); add no new rendering path. The desktop notification truncates to 200 chars.
- Files stay at or under 400 lines (`recover_hung.rs` ~330 after your change, `hung_latch.rs` under 400);
  functions under 50 lines (split the park flow into `park_never_worked` and `park_exhausted`).
- Reading the stage after `update_stage` returns a fresh value; do not reuse a stale `Stage` for the park.

## The one check you may run (once)

`cd loom && cargo test --lib orchestrator::monitor::never_worked 2>&1 | tail -30`. The crate may not compile
mid-wave (E1 and E3 write the symbols you call); if so, say so and stop. No `cargo fmt`, no clippy.

## Report

Files changed; the one check and its result; the deleted old test (name) for the integrity dispute; that no
field was added to `SessionHung` and why; lines of `hung_latch.rs`, `recover_hung.rs`, `event_handler.rs` after
your edit; any deviation from common.md's reason strings (there should be none); surprises.
