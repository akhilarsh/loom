# Merge Flow

> How a completed stage reaches its target branch

## Merge Flow

How a completed worktree stage reaches the target branch, in the order the daemon runs it, and
what every outcome leaves on disk. Knowledge stages are outside this flow: they commit to the
base directly and `complete_knowledge_stage` sets `merged: true` with no branch.

### The stage is `Completed` before any merge runs

`loom stage complete` inside a worktree verifies only. The PostToolUse broker forwards
`CompleteStage` to the daemon, whose `handle_complete_stage` (`daemon/server/control_complete.rs`)
calls `stage.try_complete(None)` and writes the stage file: `status: completed`, `merged: false`,
`completed_commit: null`. No merge has happened yet, so every merge attempt that follows sees a
stage that is already `Completed`, a terminal state the transition table refuses to leave.

### Where the first merge attempt actually happens

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
phantom guard (`commits_ahead_of` must be > 0), record `completed_commit` from the branch head,
`MergeLifecycle::reconcile_overlay`, then `attempt_auto_merge` (`orchestrator/auto_merge.rs`), which calls
`merge_stage` (`git/merge/mod.rs`): take `MergeLock`, refuse if `MERGE_HEAD` exists, `git checkout
<target>` in the main checkout, `git merge --no-ff -m "Merge loom/<id> into <target>"`.

### Outcomes

| Outcome | Stage file afterwards | Daemon |
| --- | --- | --- |
| Merge succeeded and `verify_merge_succeeded` proves ancestry | `Completed`, `merged: true`; `finish_verified_merge` reconciles the base and removes worktree and branch | continues |
| Conflicts | forced to `MergeConflict`, resolver session spawned | stays alive for the resolver |
| `git merge` or checkout failed (dirty main checkout, lock timeout, missing branch) | forced to `MergeBlocked`, `failure_info` type `InfrastructureError` with the git error as evidence | last stage: exits; `loom status` shows MERGE ERROR |
| Merge ran but ancestry cannot be verified | forced to `MergeBlocked`, reason in `failure_info` | same |
| Branch has zero commits beyond target | forced to `NeedsHumanReview`, `review_reason` says the agent never committed | exits; NEEDS REVIEW |
| Auto-merge disabled for the stage or plan | stays `Completed + !merged` by design | exits; `loom status` shows unmerged |

`persist_merge_blocked` is the single writer for the blocked outcomes and `route_to_human_review`
for the review outcome; both use `force_status_with_reason` because `Completed` has no legal exits.
Nothing on the daemon path ever writes `merged: true` without `is_ancestor_of` returning true.

### Recovery from every non-merged outcome

`loom stage merge <id>`, run from the stage worktree, accepts `MergeConflict`, `MergeBlocked`, and
`Completed + !merged` (`commands/stage/merge/preflight.rs::require_merge_state`). It re-runs
`merge_stage` and, on success, `try_complete_merge`, which tolerates an already-Completed stage.

The merge runs in the operator's main checkout. `git merge` refuses when it would overwrite
tracked modifications or untracked files, and it ALSO refuses whenever anything is staged in that
checkout's index, even on paths the merge never touches. The error lists the staged paths as
"local changes ... would be overwritten by merge", which reads as an overlap when there is none.

When agents share the main checkout, do not stash or reset their work. Wait until it is committed,
then merge the target INTO `loom/<id>` inside the stage worktree, resolve conflicts there, rerun
the stage's acceptance, commit, and run `loom stage merge <id>`; the shared checkout never holds a
conflicted merge. After a successful merge the command removes the worktree and branch unless its
cwd is inside that worktree (`orchestrator/merge_lifecycle.rs::should_defer_cleanup`), which is
where this recovery runs it; cleanup is then left to the daemon, or to the next `loom run`.

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

`MergeLock` serializes loom-driven merges with an exclusive OS advisory lock (`fs2::try_lock_exclusive`) on the stable `.loom/work/merge.lock` inode. The file is created once and never unlinked; the holder's pid and timestamp are written into it for diagnosis only. `acquire` polls every 100 ms up to the caller's timeout (30 s from `merge_stage` and the probe). Release is by `Drop` or process exit, so there is no stale-lock reclamation and a pid left in the file after a merge is not a held lock.
