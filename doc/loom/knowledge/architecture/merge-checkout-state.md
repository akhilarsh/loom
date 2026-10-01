# Merge Checkout State

> Guarded fast-forward; blocked retry

## Merge Checkout State

When the target branch is checked out in the operator's main checkout R, `advance_in_checkout` (`git/merge/checkout_apply.rs`) advances it to the merge commit `M` without a merge in R. R's `HEAD` must equal `T0` (the target tip the merge was computed against); otherwise the result is `Blocked(TargetMoved)`.

`classify` (`git/merge/checkout_state.rs`, pure) compares `git status --porcelain=v2 -z --untracked-files=all` with `git diff --name-status -z --no-renames T0 M`. A path is touched when T0..M modifies, adds or deletes it.

| Case | Action |
| --- | --- |
| No tracked change and no untracked file on a touched path | `git merge --ff-only M`; git carries the local changes |
| Only overlap: untracked files M adds, byte-identical to the blob, regular files (a symlink never counts) | remove them, then fast-forward |
| Tracked overlap, dry run clean | reapply (below) |
| Tracked overlap with a predicted conflict, an untracked file that differs, or any unmerged index entry | `Blocked(UncommittedOverlap { paths })`; R untouched |

The dry run creates `S = git stash create`, then runs `git merge-tree --write-tree --merge-base=T0 M S`; exit 0 is clean.

Reapply: `update-ref refs/loom/autostash/<id>-<unix-secs> S` records a backup ref, `git stash push` sets the work aside, `merge --ff-only M` advances, then `stash pop --index`, falling back to `stash pop`. When both pops fail the result is `Blocked(ReapplyFailed { backup_ref })`: the stash entry is kept and loom never runs `reset --hard`. `--autostash` is never used because it writes conflict markers into the checkout. A refused fast-forward restores whatever the call changed.

Blocked results are retried by the daemon (`retry_blocked_merge`, `merge_handler/blocked_retry.rs`). The retry skips an attempt when `git::merge::blocked_merge_inputs` (a hash of the target, branch and R `HEAD` ids, R's porcelain-v2 status, size and mtime of the paths it names, `git worktree list`, and the operator marker) equals the inputs of the last attempt that ended `UncommittedOverlap`, `TargetCheckedOutElsewhere` or `OperatorOperation`. The inputs are kept in memory in `Orchestrator.blocked_merge_inputs`. Every attempt writes a fresh `commit-tree` commit, and for a tracked overlap `stash create` commits, before the block is found, so retrying an unchanged blocked stage every tick would leave unreachable objects.
