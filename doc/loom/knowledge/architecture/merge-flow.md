# Merge Flow

> How a completed stage reaches its target branch

## Merge Flow

How a completed worktree stage reaches the target branch, in the order the daemon runs it, and
what every outcome leaves on disk. Knowledge stages are outside this flow: they commit to the
base directly and `complete_knowledge_stage` sets `merged: true` with no branch.

## The stage is `Completed` before any merge runs

`loom stage complete` inside a worktree verifies only. The PostToolUse broker forwards
`CompleteStage` to the daemon, whose `handle_complete_stage` (`daemon/server/control_complete.rs`)
calls `stage.try_complete(None)` and writes the stage file: `status: completed`, `merged: false`,
`completed_commit: null`. No merge has happened yet, so every merge attempt that follows sees a
stage that is already `Completed`, a terminal state the transition table refuses to leave.

## Where the first merge attempt actually happens

The main loop (`orchestrator/core/orchestrator.rs`) runs `sync_graph_with_stage_files`
(`orchestrator/core/recovery.rs`) before `monitor.poll()` on every tick. The sync finds the fresh
stage, derives `completed_commit` from the `loom/<id>` branch head, runs the ancestry check (false,
nothing is merged yet), marks the graph node Completed, and queues the stage for the "Fix 11"
one-shot retry at the end of the same function. That retry calls `try_auto_merge`
(`orchestrator/core/merge_handler.rs`), which is the first and normal merge attempt. The
`StageCompleted` monitor event, handled next in the same tick by `handle_stage_completed`
(`orchestrator/core/completion_handler.rs`), calls `try_auto_merge` a second time; by then
`merged` is true and the call only runs `cleanup_already_merged`.

`try_auto_merge` in order: auto-merge enabled check (stage, then plan, then daemon default),
`auto_merge_precheck_blocks` (when the branch exists: the merge gate, then the phantom guard,
`commits_ahead_of` must be > 0), record `completed_commit` from the branch head,
`MergeLifecycle::reconcile_overlay`, then `attempt_auto_merge` (`orchestrator/auto_merge.rs`, which
spawns no session) and `apply_auto_merge_outcome` (`orchestrator/core/merge_handler/auto_merge_outcome.rs`).
`attempt_auto_merge` calls `merge_stage` (`git/merge/mod.rs`), described next.

## `merge_stage` merges off the operator's checkout

`merge_stage(stage_id, target, repo_root, work_dir)` runs under `MergeLock`. It never checks out a
branch, runs a three-way merge, or creates `MERGE_HEAD` in the operator's main checkout R. Only when
the target is checked out in R does it change R's files: `git merge --ff-only`, plus the stash and
untracked-file removal of the reapply path:

1. An operator operation in progress in R (`MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`,
   `rebase-merge/`, `rebase-apply/`) returns `Blocked(OperatorOperation { marker })`.
2. `T0 = rev-parse <target>`, `B = rev-parse loom/<id>`. `B` already in `T0` is `AlreadyUpToDate`.
3. `merge_tree` (`git/merge/tree.rs`) runs `git merge-tree --write-tree --name-only --no-messages -z T0 B`.
   Exit 0 is clean (tree id first), exit 1 is a conflict (tree id, then the conflicted paths). Exit 1
   with empty stdout (bad revision) or any other code is an error. A conflict returns
   `Conflict { conflicting_files }` and R is untouched.
4. Clean: `commit_merge` runs `git commit-tree <tree> -p T0 -p B -m "Merge loom/<id> into <target>"`.
   The commit always has two parents, even when `B` already contains `T0` after a resolver merged the
   target in. Stats come from `git diff --shortstat T0 M`.
5. `advance_target` (`tree.rs`) moves the target to the merge commit `M`, by where the target is
   checked out (`git worktree list --porcelain`):
   - nowhere (R on another branch or detached included): `git update-ref -m "loom: merge loom/<id>"
     refs/heads/<target> M T0`. A refusal because the target moved is `Blocked(TargetMoved)`.
   - in another worktree: `Blocked(TargetCheckedOutElsewhere { path })`.
   - in R: `advance_in_checkout` (`git/merge/checkout_apply.rs`), a guarded fast-forward that keeps the
     operator's uncommitted work. See [merge-checkout-state](merge-checkout-state.md).

`MergeResult` is `Success { files_changed, insertions, deletions, backup_ref }`,
`Conflict { conflicting_files }`, `AlreadyUpToDate`, or `Blocked(MergeBlock)`. `MergeBlock` is stored on
the stage (`Stage.merge_block`, `#[serde(tag = "kind")]`) and has an operator-facing `Display`.
`loom run` refuses git older than 2.40 (`commands/run/git_preflight.rs`, called from
`run_startup_preflights`) because `merge-tree --write-tree` needs it. `git/runner.rs` gives
`merge-tree`, `commit-tree`, `update-ref` and `stash` the 120 s mutation timeout.

## Outcomes

`apply_auto_merge_outcome` maps each result:

| Outcome | Stage file afterwards | Daemon |
| --- | --- | --- |
| Success or `AlreadyUpToDate`, and `verify_merge_succeeded` proves ancestry | `Completed`, `merged: true`; `finalize_auto_merge` runs `finish_verified_merge`, which reconciles the base and removes worktree and branch; a backup ref is printed and logged | continues |
| Conflict | `record_merge_conflict` forces `MergeConflict`; nothing is spawned here | stays alive; the spawn loop starts a resolver in the stage worktree |
| Blocked (`MergeBlock`) | `record_merge_block` forces `MergeBlocked` with `Stage.merge_block` and `failure_info` carrying the block sentence as evidence, so `loom status` shows it | stays alive; the loop retries the merge |
| Git error (lock timeout, missing branch) | forced to `MergeBlocked`, `failure_info` type `InfrastructureError` with the git error as evidence | last stage: exits; `loom status` shows MERGE ERROR |
| Merge ran but ancestry cannot be verified | forced to `MergeBlocked`, reason in `failure_info` | same |
| Branch has zero commits beyond target | forced to `NeedsHumanReview`, `review_reason` says the agent never committed | exits; NEEDS REVIEW |
| Auto-merge disabled for the stage or plan | stays `Completed + !merged` by design | exits; `loom status` shows unmerged |

`persist_merge_blocked` is the writer for the infrastructure outcomes and `route_to_human_review`
for the review outcome; both use `force_status_with_reason` because `Completed` has no legal exits.
Nothing on the daemon path writes `merged: true` without `is_ancestor_of` returning true.

## Recovery from every non-merged outcome

The spawn loop `spawn_merge_resolution_sessions` runs every tick. A `MergeBlocked` stage that has a
`merge_block` goes to `retry_blocked_merge` (`merge_handler/blocked_retry.rs`) and never gets a
resolver; every other `MergeConflict` or `MergeBlocked` stage goes to `spawn_resolver_if_due`. See
[merge-and-recovery](../patterns/merge-and-recovery.md#merge-resolver-spawn-loop).

The resolver works in the stage worktree `.worktrees/<id>`, never in R: it merges the target into
`loom/<id>`, resolves, reruns the stage's acceptance, commits, and runs `loom stage merge <id>
--resolved`. The relayed request reaches `resolve_merge_from_inbox`
(`orchestrator/core/inbox_drain/merge_resolved.rs`): status check, `check_resolved_worktree`
(`git/merge/resolved.rs`: worktree on `loom/<id>`, no `MERGE_HEAD`, no unmerged path, no tracked
change, contains the current target tip; untracked files are allowed because sandboxes leave stubs),
then `land_stage_merge` (`merge_handler/landing.rs`): merge gate, `merge_stage`, record. A merged
result is applied with no cleanup, because the resolver still runs in the worktree; if the target
moved and the merge conflicts again, the request is refused with instructions and the stage stays
`MergeConflict`; a `Blocked` result is applied and the block recorded. When the resolver exits,
`handle_merge_session_completed` (`merge_handler/resolver_exit.rs`) cleans up a stage that is
`merged` and ancestry-proven, leaves a `MergeBlocked` stage with a block to the retry, and for a
`MergeConflict` stage runs `check_resolved_worktree`, `land_stage_merge`, and cleanup on success.
Otherwise the loop spawns the next counted resolver. `finalize_merge_resolution` keeps the
phantom-merge invariant (ancestry proof before `merged = true`).

The merge gate (`merge_handler/merge_gate.rs`) runs before every daemon-side merge: auto-merge,
`--resolved`, resolver exit, blocked retry. After a resolver merges the target in, the gate's
`merge-base(target, stage)..stage` diff is the stage's own work plus the resolution. The CLI paths
do not run the gate.

`loom stage merge <id>` (`merge_retry`, run from the stage worktree) accepts `MergeConflict`,
`MergeBlocked`, and `Completed + !merged` (`commands/stage/merge/preflight.rs::require_merge_state`).
It re-runs `merge_stage`: Success or `AlreadyUpToDate` complete the stage and clear the block;
Conflict moves the stage to `MergeConflict` and prints the manual route (in `.worktrees/<id>`: merge
the target, resolve, commit, `loom stage merge <id> --resolved`); Blocked records the block.
`loom stage merge --resolved` without a daemon relay (`merge_resolved`, `commands/stage/merge.rs` and
`merge/landing.rs`) takes the repo root from `WorkDir::main_project_root()`, never the cwd (the
worktree), runs `check_resolved_worktree` and `merge_stage`, and completes only after
`verify_or_derive_completed_commit`. The progressive merge in `loom stage complete` and
`loom stage human-review --approve` turns `ProgressiveMergeResult::Blocked(MergeBlock)` into
`MergeBlocked` with the block. `commands/stage/merge/finish.rs` prints why cleanup did not finish:
refused, failed, or deferred.

Worktree cleanup (`merge_lifecycle::finish_verified_merge`) for a resolved merge runs only when no
resolver runs in the worktree: at the resolver's exit, or in the blocked retry when none is live. The
CLI removes the worktree and branch unless its cwd is inside that worktree
(`orchestrator/merge_lifecycle.rs::should_defer_cleanup`); cleanup is then left to the daemon, or to
the next `loom run`.

Worktree removal: the daemon's cleanup removes loom's known scaffold files and the empty stubs a
sandbox leaves at the worktree root (`verify/tool_artifacts.rs::NAMES`,
`git/cleanup/worktree.rs::remove_sandbox_stubs`), then runs a non-forced `git worktree remove`,
which refuses on any remaining modified or untracked file. `loom worktree remove <id>` checks
`git status` first (`git/cleanup/removal.rs::require_clean_worktree`) and refuses on modified
tracked files and on untracked files other than that scaffold and those stubs. Both delete ignored
files with the worktree, `target/` and a generated `REVIEW-PLAN-*.md` included, so move a review
into the main `doc/plans/` first. From a sandboxed session git cannot delete `.git/worktrees/<id>`
(`Device or resource busy`); run `git worktree prune` from an operator shell.

## Merge Lock (git/merge/lock.rs)

`MergeLock` serializes loom-driven merges (the daemon's auto-merge, resolved landing and blocked retry, and the CLI merge paths) with an exclusive OS advisory lock (`fs2::try_lock_exclusive`) on the stable `.loom/work/merge.lock` inode. The file is created once and never unlinked; the holder's pid and timestamp are written into it for diagnosis only. `acquire` polls every 100 ms up to the caller's timeout (30 s from `merge_stage`). Release is by `Drop` or process exit, so there is no stale-lock reclamation and a pid left in the file after a merge is not a held lock.
