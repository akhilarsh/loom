---
---
# Merge And Recovery

> Progressive merge, conflict recovery

## Progressive Merge Pattern

Dependencies merged to main before dependent stages execute: `Stage A completes -> Merge A to main -> Stage B starts`. Base branch resolution: no deps = init_base_branch or default; all deps merged = main; single dep not merged = dependency branch (legacy fallback). MergeLock prevents concurrent merges (30s timeout, 5min stale cleanup).

## Merge Anti-Respawn Pattern

A dead resolver is respawned only within a budget: `MAX_MERGE_RESOLVER_ATTEMPTS` (3) per stage, counted in `.loom/work/merge-resolver-attempts/<stage-id>.count` (`orchestrator/core/merge_handler/resolver_attempts.rs`). `ReservedAttempt::record` writes count+1 before a spawn; `keep()` runs as soon as the backend spawn returns `Ok`, and dropping the guard without it rolls the count back, so only a spawned resolver consumes budget. A counter file that exists but cannot be read or parsed, or a counter path that is not a regular file, reads as the cap; a counter that cannot be written refuses the spawn and routes the stage to review.

After a backend `Err`, `settle_resolver_spawn` (`resolver_stop.rs`) kills the would-be session and asks `teardown_proves_gone` whether it is gone. On tmux the kill must succeed and the liveness probe must say gone, because a surviving server on the socket shows only as a kill failure. On the native lane the probe decides alone: it checks the `loom-merge-<stage>` window as well as the PID file, and a lane with no terminal launches nothing. Proven gone refunds the attempt, and the failure is retried next tick. Not proven keeps the attempt and routes the stage to review. A `save_session` failure after a successful spawn only warns: the session stays in `active_sessions`, which holds the stage while its PID identity lives.

The merge signal file is a liveness record, never a respawn guard: `find_live_merge_session_for_stage` (`orchestrator/signals/merge.rs`) deletes it once its session is dead. A readable signal whose session record is missing is stale, because failed spawns, orphan recovery and `loom sessions kill` all leave record-less signals behind. A record that exists but cannot be read is an error, which routes the stage to review. An unreadable signal is attributed through the session record its filename names: another stage's is skipped, this stage's blocks the spawn while alive and is removed when dead, and one that cannot be attributed routes the stage to review.

## Merge Recovery Flow [UPDATED 2026-04-27]

MergeConflict -> bail\!() forces original session to exit -> commit-guard.sh allows exit for MergeConflict status -> detection.rs recognizes as normal exit -> spawn_merge_resolution_sessions() kills any stale original session, then spawns resolver -> merge signal includes "Inherited Responsibilities" section explaining resolver owns the stage -> user directed to `loom stage merge <stage-id> --resolved`.

Key invariant: the original execution session MUST exit when merge conflict is detected. Three mechanisms enforce this:

1. `bail\!()` in `complete_with_merge()` propagates error and terminates the session
2. `commit-guard.sh` does NOT block exit for MergeConflict status
3. `spawn_merge_resolution_sessions()` actively kills stale sessions before spawning resolver

**Daemon ordering invariant (2026-04-27):** Reconciliation runs BEFORE `sync_graph_with_stage_files` AND BEFORE `recover_orphaned_sessions`. Recovery deletes orphaned merge session files; attribution depends on their metadata. Sync reads stage files into the graph; if reconcile flips disk state AFTER sync, the graph keeps the stale view and would queue dependents based on a phantom merge.

**Daemon-off CLI parity (2026-04-27):** `loom stage complete` on a `Completed + merged=true` stage with an active main-repo merge attributed to it triggers the same revert the daemon performs (`Completed → MergeConflict + merged=false + merge_conflict=true`) before spawning the resolver. The router's `RevertAndSpawnResolver` arm encodes this; persistence is the caller's responsibility, BEFORE spawn so `spawn_merge_resolver`'s status contract is satisfied.

## Attribution-Aware Recovery (2026-04-27)

`MERGE_HEAD` in the main repo is global state — only one merge in progress at a time across all stages. Stage-state mutation triggered by detecting it must come with proof of attribution; without proof, refuse rather than mutate.

Three attribution sources (first match wins):

1. **MergeSession metadata** — orphaned or live `SessionType::Merge` with matching `merge_source_branch`.
2. **Branch HEAD match** — a `MERGE_HEAD` SHA equals `loom/<stage-id>` HEAD.
3. **Completed-commit match** — a `MERGE_HEAD` SHA equals `stage.completed_commit`.

**BaseConflict carve-out:** When current HEAD is `loom/_base/*` (or any session has `SessionType::BaseConflict` matching it), return `GlobalUnattributed` even if the merge heads contain a stage branch's commit. Multi-dependency base merges check out their own branch and run a merge there; their MERGE_HEAD must NOT mutate stage state.

Single decision point: `attribute_main_repo_merge` in `orchestrator/merge_attribution.rs`. Both daemon recovery (`reconcile_main_repo_active_merge`) and the CLI router consume it.

## Pure Routing Helper (2026-04-27)

`route_complete_for_conflicts` is the canonical example: read-only function that returns `CompleteConflictRoute` without writing to disk. Persistence is the caller's responsibility on the success path only. This preserves the "refusal preserves stage file state" invariant — refusal always leaves the stage file untouched, which is critical for tests and for users investigating why a completion attempt was rejected.

Apply this pattern when adding routing/verification helpers: keep the function pure, return an enum of decisions, let the caller persist on the success branch.

## No Worker Thread for Adjudication (Retired Pattern)

**Retired 2026-08-31. Do not reintroduce this for adjudication.** This section described a
worker-thread + mpsc channel the adjudicator used to drive a blocking call off the orchestrator's
poll loop: `worker_completion_tx`/`_rx` on the `Orchestrator`, a `std::thread::spawn` per dispute,
and a drain in the main tick. All of it is deleted: the adjudication worker and
client modules were removed outright, so there is no thread to reintroduce it into.

Adjudication now spawns a real loom session through the `TerminalBackend` and observes the
resulting state change on the ordinary poll loop, the same way merge conflict resolution has always
worked. That removed the thread registry, the mpsc channel, the cooperative cancellation flag, the
`.inflight` staleness marker, and the retry/backoff loop in one go — a session is already a tracked,
restart-surviving, externally-observable unit of work, and the thread machinery was reimplementing a
worse version of it.

The general lesson, if you are reaching for a worker thread in the orchestrator: ask first whether
the work is a SESSION. Loom already knows how to spawn one, track its liveness across a daemon
restart, and notice when the state it was supposed to change has changed. A thread gets you none of
that and costs you a shutdown story.

## Dispute File Authority Split Pattern

Three-file trust boundary to prevent self-approval attacks:

| File             | Writer                             | Content                  | Rationale                                            |
| ---------------- | ----------------------------------- | ------------------------ | ---------------------------------------------------- |
| `.loom/work/disputes/<stage-id>/<n>/request.md`     | Daemon (on agent's behalf via RPC) | Agent's evidence payload | Agent can read but never write directly              |
| `.loom/work/disputes/<stage-id>/<n>/verdict.md`     | Daemon worker thread only          | Verdict + citations      | Stage agents never write here — daemon-authored only |
| `.loom/work/disputes/<stage-id>/<n>/applied.marker` | Daemon only (zero-byte)            | Idempotency guard        | Prevents re-application on restart                   |

If the agent could write both request and verdict, it could pre-fill `verdict: Accept` and self-approve. The split enforces the trust boundary at the filesystem level.

## Plan Amendment Atomic Write Pattern

For amending the IN_PROGRESS plan file safely (Stage 3):

```text
1. Acquire .loom/work/plan_versions/.lock  (file lock — serializes concurrent amendments)
2. Compute new plan content in memory
3. Atomic-write .loom/work/plan_versions/<n>.md  (full snapshot)
4. Append to .loom/work/plan_versions/audit.md  (O_APPEND — atomic for small rows)
5. Atomic temp+rename of IN_PROGRESS plan file to new content
6. Release lock
```

Recovery on crash: scan audit.md for latest amendment; verify plan file matches snapshot. If mismatch → restore from `<n>.md`. If `<n>.md` missing → discard audit row, use `<n-1>.md`.

Note: `plan/graph/loader.rs:60-86` PREFERS `.loom/work/stages/` files over the plan file. Plan-file amendment MUST also update the corresponding `.loom/work/stages/<stage_id>.md` for the change to be reflected in the running orchestrator graph.

## Merge Resolver Spawn Loop

`MergeConflict` and `MergeBlocked` never count as terminal for the watch-mode exit (`recovery.rs::stage_file_is_terminal`), so the daemon stays up until each such stage has a resolver running or reaches `NeedsHumanReview`. Every tick `spawn_resolver_if_due` (`orchestrator/core/merge_handler/resolver_spawn.rs`) takes each merge-state stage through, in order:

1. the merge gate: a branch touching a control path has any live resolver stopped (`resolver_stop.rs::stop_gated_resolvers`), then goes to review with a note that the main checkout may hold an in-progress merge;
2. stale-session cleanup, then the live-signal check;
3. a strict existence check on the stage branch and the target branch: missing goes to review, a git failure is transient;
4. the resolver cap, which escalates to review;
5. a fresh re-read of the stage, the attempt reservation, and the spawn.

A transient spawn failure (dirty main checkout, tmux, lock timeout) is retried every tick with no cap, and the daemon stays up meanwhile; this is an operator decision. A pass works from a stage copy that can go stale while it runs git, so every routing goes through `route_merge_stage_to_review` (`review_route.rs`), which re-checks the status inside the `update_stage` lock and writes nothing when a concurrent `loom stage merge` has already moved the stage on. `route_to_human_review` writes whatever the status and is kept for `try_auto_merge`, whose stage is `Completed`.

`loom stage merge` hitting a conflict moves a `MergeConflict`/`MergeBlocked` stage to `MergeConflict` (the `MergeBlocked -> MergeConflict` edge exists for this), clears its `failure_info`, and prints what the daemon will do next, read from the resolver counter (`commands/stage/merge/next_step.rs`). A `Completed` stage that was never merged (auto-merge disabled) is left untouched and gets only the manual steps.
