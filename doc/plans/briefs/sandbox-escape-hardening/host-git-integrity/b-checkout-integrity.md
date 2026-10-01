# B: checkout-session integrity (D7) and the fail-closed merge gate

Stage `host-git-integrity`, wave 1, tier opus. Read `../common.md` first (D1, D2, D7).

## Why

A checkout-rooted session (a knowledge stage, a merge-resolution session) runs in the main
checkout `R` and keeps write access to `R/.git` by design (D1). From there it can move refs,
add `refs/replace/*`, swap a loose object for other content under the same name (git serves it
silently; only a full `git fsck` notices), plant `objects/info/alternates`, `info/grafts`, a
forged pack index or commit-graph, or a `commondir` file that redirects all git in `R` to
another repository (confirmed this session). The daemon then accepts the session's result
without looking. Separately, the merge gate and the commits-ahead probe proceed when git
cannot answer (`merge_gate.rs::merge_gate_blocks`, "failed to compute changed paths;
proceeding with merge attempt"; `merge_handler.rs::try_auto_merge`, "commits_ahead_of probe
failed; proceeding with merge attempt").

## Files you own (relative to `loom/`)

New: `src/git/checkout_integrity.rs`, `src/git/checkout_integrity/verify.rs`,
`src/git/checkout_integrity/state.rs`, `src/git/checkout_integrity/store.rs`,
`src/git/checkout_integrity/tests.rs`, `src/git/merge/control_paths.rs`.
Edited: `src/git/mod.rs`, `src/git/merge/mod.rs`,
`src/orchestrator/core/merge_handler.rs`,
`src/orchestrator/core/merge_handler/merge_gate.rs`,
`src/orchestrator/core/merge_handler/merge_gate_tests.rs`,
`src/daemon/server/control_complete.rs`, `src/daemon/server/control_complete_tests.rs`,
`src/daemon/server/completion_dispatch/tests.rs`,
`src/orchestrator/core/inbox_drain/tests_merge.rs`,
`src/orchestrator/terminal/backend.rs`.

The last four are outside `common.md`'s ownership map; the plan's stage owns them. Worker A
owns the rest of `src/git/**` in the same wave; you need nothing from A to compile.

## 1. `loom::git::checkout_integrity` (the public surface the contracts call)

```rust
/// What a checkout-rooted session could change in the main repository's git
/// directory, recorded when the daemon spawns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDirSnapshot {
    /// `ref: <name>` for a symbolic HEAD, else the detached object name.
    pub head: String,
    /// Every ref `git for-each-ref` lists: name -> object name.
    pub refs: BTreeMap<String, String>,
    /// Every regular file in `objects/pack/`: file name -> "<size>:<sha256>".
    pub packs: BTreeMap<String, String>,
    /// Each control file, relative to the git directory: sha256, or None when absent.
    pub control_files: BTreeMap<String, Option<String>>,
    /// Every path `git diff --cached --name-only -z` lists: path -> its
    /// `git ls-files -s -z -- <path>` row.
    pub staged: BTreeMap<String, String>,
}

pub fn snapshot(repo_root: &Path) -> anyhow::Result<GitDirSnapshot>;
pub fn differences(repo_root: &Path, before: &GitDirSnapshot) -> anyhow::Result<Vec<String>>;
pub fn record(work_dir: &Path, repo_root: &Path, stage_id: &str, kind: SessionType)
    -> anyhow::Result<()>;
pub fn verify_recorded(work_dir: &Path, repo_root: &Path, stage_id: &str, kind: SessionType)
    -> anyhow::Result<Vec<String>>;
```

`checkout_integrity.rs` holds the type, `snapshot`, the constants and the module docs;
`verify.rs` holds `differences`; `store.rs` holds `record`/`verify_recorded`. Re-export the
four functions and the type from `checkout_integrity.rs`. Declare `pub mod
checkout_integrity;` in `src/git/mod.rs`. Every file under 400 lines, every function under 50.

Control files, compared exactly: `commondir`, `gitdir`, `config.worktree`, `shallow`,
`info/grafts`, `info/attributes`, `objects/info/alternates`, `objects/info/http-alternates`.

### Git runs pinned to `R/.git`

Every git call here goes through `crate::git::runner::run_git_with_env(args, &[("GIT_DIR",
G), ("GIT_COMMON_DIR", G)], repo_root)` with `G` the canonical `repo_root/.git`, which must be
a directory (`symlink_metadata`), else `Err`. `GIT_COMMON_DIR` is what makes git ignore a
planted `commondir`; `GIT_DIR` alone does not (verified on git 2.53).

### `snapshot`

- `Err` when `G/commondir` or `G/gitdir` exists: a main repository's git directory never holds
  either, so a baseline must not absorb one.
- `head`: `symbolic-ref -q HEAD` (exit 0 gives `ref: <name>`; exit 1 means detached, then
  `rev-parse HEAD`).
- `refs`: `for-each-ref --format=%(refname)%00%(objectname)`.
- `packs`: size and SHA-256 of each regular file in `G/objects/pack` (hash with `sha2`, already
  a dependency; read in chunks, never whole files into memory). 30 MB of packs hashed in 0.02 s
  on this repository.
- `control_files`: SHA-256 of each file's content, `None` when absent.
- `staged`: `diff --cached --name-only -z` (index against `HEAD`; it reads no working-tree file,
  so no filter runs), then one `ls-files -s -z -- <paths>` for the rows.

### `differences`, in this order

1. Control files. Each difference is one entry: "`<path>` was created", "was removed",
   "changed". When any exists, return them now plus the entry "the git directory's redirect
   files changed; refs and objects were not checked", without running git.
2. Operations left in progress: each of `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`,
   `BISECT_LOG`, `sequencer/`, `rebase-merge/`, `rebase-apply/` that exists in `G` at verify is
   an entry ("`MERGE_HEAD` exists: an operation is still in progress in the main checkout"),
   whatever the snapshot held. A planted `MERGE_HEAD` turns the operator's next plain `git
   commit` in `R` into a merge commit that grafts history. For a merge session this is checked
   at acceptance, after its merge commit: verify that the relayed path consumed `MERGE_HEAD`
   first (`inbox_drain/merge_resolved.rs::merge_is_concluded` refuses while it exists, before
   `finalize_merge_resolution` runs) and that the process-exit path
   (`handle_merge_session_completed`) reaches `finalize_merge_resolution` only once the merge
   commit exists. A `MERGE_HEAD` still present then is a second, unfinished operation and is
   reported. If either path can reach `finalize_merge_resolution` with the session's own
   `MERGE_HEAD` legitimately present, move the check after the daemon's own finalize step
   instead and say so in your report. A knowledge completion runs on the daemon's server
   thread while the orchestrator may be merging another stage in `R`: read
   `git/merge/lock.rs::MergeLock`, and when the daemon's merges hold it, take it around
   `verify_recorded` there so a transient `MERGE_HEAD` of the daemon's own merge is never
   seen; report what you found.
3. HEAD: a symbolic HEAD must name the same ref; a detached one must still be detached.
4. Refs: every ref except the branch `before.head` names and every ref under
   `refs/heads/loom/` must be unchanged: "ref `<name>` moved from `<a>` to `<b>`",
   "ref `<name>` was created at `<b>`", "ref `<name>` was deleted (was `<a>`)". The target may
   move and may be created (an unborn branch), never deleted. `refs/heads/loom/**` is excluded
   because the daemon and other stage sessions write it while a checkout session runs; a
   checkout session writes its target directly, so moving a stage ref gives it no content path
   it lacks.
5. The index: the staged set at verify must be a subset of `before.staged`, each path with an
   identical `ls-files -s` row. Every other staged path is an entry ("`<path>` is staged in the
   main checkout's index and was not at the snapshot"): the operator's next plain `git commit`
   in `R` would commit it. A session that commits leaves nothing staged, which passes.
6. Packs: for each pack whose `.pack` or `.idx` is new or changed since the snapshot, `git
   verify-pack <G>/objects/pack/<base>.idx`; a failure is "`objects/pack/<base>.idx` fails git
   verify-pack: <first stderr line>". A new or changed `multi-pack-index`: `git
   multi-pack-index verify`. A removed pack is no difference (the daemon's own `gc --auto` after
   its relayed commits repacks while a checkout session runs); a missing reachable object
   shows in step 8.
7. When `G/objects/info/commit-graph` or `G/objects/info/commit-graphs` exists: `git
   commit-graph verify` (0.02 s here).
8. `git fsck --no-full --no-dangling --no-reflogs --no-progress` (0.09 s here): a non-zero exit
   gives one entry per output line starting `error:`, `missing` or `broken`, at most 20, then
   "and N more". It re-hashes every loose object: a swapped one prints
   `hash-path mismatch` and `missing blob <oid>` (verified; `--connectivity-only` misses it).

### `record` and `verify_recorded` (`store.rs`)

- Records only when `repo_root` canonicalizes to the same path as
  `crate::fs::work_dir::WorkDir::new(work_dir)?.repo_root()`; any other root (a worktree-rooted
  session, which D1 covers) returns `Ok(())` and writes nothing. Records only for kinds
  `SessionType::Merge`, `SessionType::BaseConflict` and `SessionType::Knowledge`.
- File: `<work_dir>/checkout-integrity/<stage-id>-<kind>.json` (kind by `SessionType`'s
  `Display`), `stage_id` checked with `crate::validation::validate_id`. Written with
  `crate::fs::locking::{locked_dir_update, atomic_write_locked}`. An existing file is KEPT: the
  first checkout session of that stage and kind in this run is the baseline, so a session that
  tampers and then crashes or hands off is still judged against the state before it. `loom
  init` removes the whole work directory, so nothing outlives the run.
- `verify_recorded`: a missing record is the single difference "no checkout-integrity snapshot
  was recorded for stage `<id>` (`<kind>`) when its session started"; otherwise
  `differences(repo_root, &record)`.

## 2. Fail-closed merge gate (`src/git/merge/control_paths.rs`)

Move `control_path_violation`, `is_control_path`, `hooks_dir_prefix`, `read_hooks_path_scope`
and `resolve_hooks_dir_prefix` from `merge_gate.rs` verbatim (doc comments included) into
`control_paths.rs` as `pub(crate)`, declare `pub mod control_paths;` in `git/merge/mod.rs`,
and add:

```rust
/// Why a merge of `stage_branch` into `target_branch` needs human review:
/// its diff since it split from the target touches a control path, or that
/// diff cannot be computed. `None` lets the merge proceed.
pub fn merge_refusal(repo_root: &Path, target_branch: &str, stage_branch: &str)
    -> Option<String>
```

An error reads "the merge gate could not list the paths `<stage_branch>` changes since it split
from `<target_branch>` (<error:#>); review the branch before it merges".

In `merge_gate.rs`: `merge_gate_blocks` becomes `let Some(reason) = merge_refusal(...) else {
return false; }; self.route_to_human_review(stage_id, reason, None); true`. Keep the moved
names reachable for `merge_gate_tests.rs`, which uses them through `use super::*` and must not
change: add `#[cfg(test)] use crate::git::merge::control_paths::{control_path_violation,
hooks_dir_prefix, is_control_path, resolve_hooks_dir_prefix};` and keep in scope every other
name those tests take from `super` (`Path` among them; a `#[cfg(test)]` import when the file no
longer uses it). Update the module doc's call-site list.

## 3. Wiring

- `src/orchestrator/terminal/backend.rs::SessionBackend::spawn_main_repo_session` (the one
  dispatch every Merge, Knowledge and Adjudication spawn goes through, `auto_merge.rs` and
  `merge_handler.rs::spawn_merge_resolution_session` included): before `dispatch_spawn`,
  `crate::git::checkout_integrity::record(&self.work_dir, repo_root, &stage.id, kind)?;`. A
  failure refuses the spawn (the callers already treat a spawn error as a retry or a block).
- `merge_gate.rs`: add `pub(super) fn checkout_integrity_holds(&mut self, stage_id: &str) ->
  bool` in its `impl Orchestrator`: `verify_recorded(&self.config.work_dir,
  &self.config.repo_root, stage_id, SessionType::Merge)`; empty is `true`; differences route
  the stage with `route_to_human_review(stage_id, format!("the merge-resolution session changed
  the main repository's git directory outside its target branch: {}", list.join("; ")), None)`
  and return `false`; an `Err` routes with "could not verify ... (<error:#>)" and returns
  `false`.
- `merge_handler.rs::finalize_merge_resolution` (ledgered at 97 lines, file at 1267: both must
  stay EXACT). Delete its three comment lines (the two lines "Derive completed_commit from the
  branch HEAD if missing ..." and "// Safe to unwrap: ensured Some above.") and insert, right
  after the closing brace of the `match verify_merge_succeeded(...)` block, exactly:

      if !self.checkout_integrity_holds(stage_id) {
          return false;
      }

  Both callers (`handle_merge_session_completed`, `inbox_drain/merge_resolved.rs`) already
  treat `false` as "not finalized"; the relay answer's wording ("no ancestry proof") is not
  changed here and the stage's review reason carries the real one.
- `merge_handler.rs::try_auto_merge` (ledgered at 214): replace the eight-line `Err(e) => {
  tracing::warn!(...); }` arm of the `commits_ahead_of` match with exactly eight lines:

      Err(e) => {
          let reason = format!(
              "cannot count the commits {stage_branch} holds beyond {target_branch} \
               ({e:#}); the merge waits for review"
          );
          self.route_to_human_review(stage_id, reason, None);
          return false;
      }

  Add no `use` line to `merge_handler.rs`.
- `src/daemon/server/control_complete.rs::handle_complete_stage`: after
  `check_completion_gates`, call a new `fn require_intact_checkout(work_dir: &Path, stage_id:
  &str) -> Result<()>`: for a `StageType::Knowledge` stage, `verify_recorded(work_dir,
  repo_root, stage_id, SessionType::Knowledge)` with `repo_root` from
  `WorkDir::new(work_dir)?.repo_root()`; an `Err` becomes one difference; with differences,
  `update_stage` sets `force_status_with_reason(StageStatus::NeedsHumanReview, &reason)` and
  `review_reason = Some(reason)`, then `bail!("{reason}")` before the completion lock is taken.
  Verify how the orchestrator picks up a status the server writes by reading how
  `daemon/server/dispute.rs` or `self_service` moves a stage, and match it.

## 4. Test fixtures that complete without a spawn

A missing record is a difference, so three existing tests that reach acceptance without going
through `spawn_main_repo_session` need one setup line each (setup only: no assertion line
changes, `TI-edit` otherwise):

- `control_complete_tests.rs::a_knowledge_stage_completes_merged_through_the_broker`: before
  `fixture.complete(...)`, `record(&fixture.work, <repo>, "notes", SessionType::Knowledge)`
  with `<repo>` = `fixture.work` two levels up (`scratch_git_fixture` puts the work dir at
  `<repo>/.loom/work`).
- `completion_dispatch/tests.rs::Fixture::new`: record for `STAGE`, `SessionType::Knowledge`,
  at the end of setup.
- `inbox_drain/tests_merge.rs::repository`: record for `STAGE`, `SessionType::Merge`, as its
  last line.

Run `rg -n 'finalize_merge_resolution|handle_complete_stage' src --type rust` and check every
other test that reaches either; report any you find instead of editing it.

## Tests to add

`checkout_integrity/tests.rs` (TempDir repositories, isolated git config):

- `a_second_record_keeps_the_first_snapshot`
- `a_legitimate_new_pack_and_moved_stage_refs_pass` (commit on the target, move
  `refs/heads/loom/x`, `git repack -a -d`: no difference)
- `a_forged_pack_index_is_reported` (after a snapshot, repack, then flip one byte in the new
  `.idx` at offset `8 + 256*4 + 3`: `verify-pack` fails "validation error", verified)
- `a_missing_record_is_a_difference`
- `snapshot_refuses_a_git_dir_holding_commondir`
- `fsck_output_is_capped_at_twenty_entries`
- `a_leftover_rebase_directory_is_reported` (`G/rebase-merge/` created after the snapshot)
- `a_change_staged_before_the_snapshot_and_left_alone_passes` (a path staged at the snapshot,
  still staged with the same row: no difference; the same path restaged with other content:
  one difference)

The stage contract `a-planted-merge-head-or-staged-change-is-reported` covers a planted
`MERGE_HEAD` and a newly staged path through `differences`.

`merge_gate_tests.rs` (append; existing tests unchanged):
`an_unreadable_branch_diff_routes_to_human_review` (delete the stage commit's loose tree object:
`merge-base` still answers, `diff` fails with "unable to read tree", verified; `try_auto_merge`
returns `false` and the stage is `NeedsHumanReview`),
`a_merge_session_that_moved_another_ref_is_not_finalized` (record, create a tag, call
`finalize_merge_resolution`: `false`, stage `NeedsHumanReview`, reason names the tag).

`control_complete_tests.rs`:
`a_knowledge_completion_after_a_moved_ref_needs_human_review`.

## Traps

- `for-each-ref` does not list pseudo-refs (`MERGE_HEAD`, `ORIG_HEAD`, `AUTO_MERGE`): step 2
  checks the in-progress files by existence; `ORIG_HEAD` and `AUTO_MERGE` are left alone (a
  finished merge leaves them, and no later command acts on them).
- Worker A makes the runner set `GIT_NO_REPLACE_OBJECTS=1` on every git call. That hides
  replacements from object reads, not from `for-each-ref`, so a planted `refs/replace/*` still
  shows in step 4; do not rely on either behaviour in a test that runs before A's change lands.
- Keep each step of `differences` its own function so none passes 50 lines, and split
  `verify.rs` (for example the index and in-progress checks into `checkout_integrity/state.rs`,
  which you own) before it passes 400.
- `git fsck` exit codes above 0 are errors; warnings print with exit 0 and are no difference.
- Do not call worker A's `WorktreeGit::for_checkout` here: `R` is known, and the explicit
  `GIT_DIR`/`GIT_COMMON_DIR` env is what defeats a planted `commondir`.
- Adjudication sessions are recorded nowhere and verified nowhere in this stage (the plan
  records why); `record` skips `SessionType::Adjudication`.

## Check

Once: `cargo test --lib git::checkout_integrity::`. Report compile errors in files you do not
own with file:line.
