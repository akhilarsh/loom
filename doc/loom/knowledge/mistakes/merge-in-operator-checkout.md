# Merge In Operator Checkout

> Git in operator checkout; plan errors

## Merges Ran Git in the Operator's Checkout (2026-10-02)

**What happened**: `merge_stage` checked out the target and ran `git merge --no-ff` in the operator's main checkout, the conflict probe did a checkout and `merge --no-commit` there, and the resolver worked there. Merge resolvers in another project failed on 2026-10-01 with `unable to unlink old 'vitest.config.ts': Device or resource busy`, and one aborted merge left 11 untracked branch files in the checkout. Merges also blocked on uncommitted files the merge never touched. Earlier incidents of the same cause: [an aborted sandboxed merge leaves untracked files](sandbox-tooling-and-network.md#a-sandboxed-git-merge-that-aborts-still-leaves-the-branchs-new-files-untracked-2026-09-10), [a bind mount breaks git merge and git stash](sandbox-tooling-and-network.md#a-sandbox-bind-mount-makes-git-merge-and-git-stash-fail-in-an-interactive-session-2026-09-13), [merge with merge-tree and commit-tree](phantom-merges.md#git-merge-fails-on-sandbox-bind-mounted-files-merge-with-merge-tree-and-commit-tree-2026-09-19).

**Why**: the merge used the operator's working tree as scratch space, so every sandbox artefact, dirty file or interruption there became a merge failure.

**Prevention**: no agent runs git against the main checkout. The merge is computed with `merge-tree` and `commit-tree`, and the main checkout is touched only by a guarded fast-forward ([merge-flow](../architecture/merge-flow.md), [merge-checkout-state](../architecture/merge-checkout-state.md)).

**Fix**: `merge_stage` rewritten around `merge-tree`/`commit-tree`/`advance_target`, the resolver moved to the stage worktree, blocked merges retried by the daemon (commits f42b4eb0, 0f2d65a3, 5a91759d, 1bd5804b, a731e3c2).

## Plan Errors Found While Implementing the Merge-Off-Checkout Plan (2026-10-02)

**What happened**: six defects in `PLAN-merge-off-main-checkout`.

1. The integration scenario said an overlapping uncommitted edit "merges on the tick after the edit is committed". Committing an edit the dry run predicted to conflict turns the next attempt into a `Conflict` (same lines, same three-way result). The test sets the edit aside with `git stash` instead.
2. W3a and W3b were declared parallel with disjoint files, but W3a changed `generate_merge_signal` and `spawn_merge_resolver` signatures that W3b's files call, so they ran in sequence.
3. The per-tick retry of a blocked merge wrote a new commit object (and stash commits) every tick before discovering the block. The input fingerprint (`blocked_merge_inputs`) fixed it.
4. The plan's "report the backup ref in the stage's merge note" names a field that does not exist.
5. `git merge-tree` with a bad revision exits 1 with empty stdout, not 128.
6. A web fixture (`web/src/api/fixtures/snapshot.json`) pinned the old cap text "attempt 1 of 3" and was not in the plan's file list.

**Why**: the plan was written from the intended behaviour without checking each step against the merge algebra, the callers of changed signatures, or what a per-tick retry writes.

**Prevention**: when a plan pins an integration scenario, check that its steps are consistent with the merge algebra it relies on. When units are declared parallel, check the signatures each one changes against the callers in the other's files. When a design retries every tick, ask what each attempt writes. Search fixtures and snapshots for text a change alters.

**Fix**: each item corrected in the implementation as listed.
