# X: single-file pinning conversions (codex lane)

Stage `host-git-integrity`, wave 2 (after worker A returns), lane codex via
`loom-codex-forwarder`, `--model gpt-5.6-terra --effort xhigh`. One unit is one file. Batches:
X1-X6 in one message, then X7-X10 once those return. The main agent forwards each unit as its
own forwarder with the prompt "Read
doc/plans/briefs/sandbox-escape-hardening/host-git-integrity/x-codex-units.md: the section
Shared rules, then unit X<n> only. Change only the file that unit names."

## Shared rules

- Edit only your unit's file. Run no git, touch no `.loom` path, write no test that builds a
  repository. Do not run cargo: the main agent runs the unit's command after you return.
- Worker A added these to `loom::git` (read their doc comments in
  `loom/src/git/worktree/pinned.rs` and `loom/src/git/branch/{status,ancestry}.rs` first):
  - `WorktreeGit::pinned(repo_root: &Path, worktree: &Path) -> Result<WorktreeGit>`: use it
    when the function already holds the repository root.
  - `WorktreeGit::for_checkout(checkout: &Path) -> Result<WorktreeGit>`: a checkout ROOT
    (a stage worktree or the main checkout) and nothing else in hand.
  - `WorktreeGit::for_dir(dir: &Path) -> Result<WorktreeGit>`: a directory somewhere inside a
    checkout (an acceptance directory).
  - `WorktreeGit::run_read_only(&self, args: &[&str]) -> Result<Output>`,
    `read_checked(&self, args) -> Result<String>` (fails on non-zero exit, trimmed stdout),
    `work_tree(&self) -> &Path`.
  - `crate::git::branch::{has_uncommitted_changes_in, list_working_tree_changes_in,
    commits_ahead_of_in, is_ancestor_of_in}`, each taking `&WorktreeGit` first.
- Why: git that discovers its directory through a stage worktree's `.git` file (an
  agent-writable pointer) reads the agent's configuration, whose clean filter `git status`
  runs on the host. Pinned git never reads that file.
- Keep every existing error path's outcome (the "answers no", "no evidence", "refusal" the
  function already gives on a git failure). Keep line length under 100 columns.
- Ledgered items must keep their exact line counts (`loom/maintainability-baseline.txt`):
  the unit says when. Count lines before and after your edit.

## X1: `loom/src/orchestrator/monitor/parked.rs`

`finished_in_worktree(worktree, branch, base)`: `let Ok(git) =
WorktreeGit::for_checkout(worktree) else { return false; };`, then `commits_ahead_of_in(&git,
branch, base)` and `has_uncommitted_changes_in(&git)` in place of the path forms. Update the
module doc's link `[`has_uncommitted_changes`]` to the `_in` name. Existing tests must pass
unchanged (`a_failed_git_probe_answers_no` passes `/nonexistent/worktree`: `for_checkout` fails
on it, which answers no).
Command: `cargo test --lib orchestrator::monitor::`

## X2: `loom/src/handoff/session_content.rs`

`build_session_content` and `modified_files` run git in `handoff.checkout` (the session's
worktree, or the repository root for a knowledge session). Resolve `let git =
WorktreeGit::for_checkout(handoff.checkout).ok();` once in `build_session_content`; the
branch is `git.read_checked(&["rev-parse", "--abbrev-ref", "HEAD"]).ok()`, replacing
`current_branch(handoff.checkout).ok()`; `modified_files` takes `Option<&WorktreeGit>` and runs
`run_read_only(&["status", "--short"])`, empty when `None` or on failure, as today. Drop the
now-unused imports.
Command: `cargo test --lib handoff::`

## X3: `loom/src/verify/before_after.rs`

`find_prior_stage_work(stage_branch, base_branch, repo_root, worktree_path)`: the working-tree
probe becomes `WorktreeGit::pinned(repo_root, worktree_path)` then
`list_working_tree_changes_in(&git)`. A pin failure takes the existing `Err(e)` arm (the same
`tracing::warn!` and `None`). Update the doc comment: the probe reads the worktree through its
registered git directory. `commits_ahead_of` there already runs in `repo_root`; leave it.
Command: `cargo test --lib verify::before_after`

## X4: `loom/src/verify/criteria/cache_fingerprint.rs`

`capture_with_budget`: replace `resolve_repo_root(acceptance_dir)` with `let git =
WorktreeGit::for_dir(acceptance_dir).ok()?; let repo_root =
git.work_tree().to_path_buf();`. `head`: `git.read_checked(&["rev-parse", "HEAD"]).ok()?`.
`git_bytes(args, &repo_root)` becomes `git_bytes(&git, args)` over `git.run_read_only(args)`,
same size cap and success check. Delete `resolve_repo_root` and the unused runner imports.
The rest of the function (the `starts_with` check, hashing) is unchanged.
Command: `cargo test --lib verify::criteria::`

## X5: `loom/src/orchestrator/merge_lifecycle/containment.rs`

`uncontained_worktree_head(stage_id, repo_root, target_branch)`: `is_ancestor_of("HEAD",
target_branch, &worktree)` becomes `WorktreeGit::pinned(repo_root, &worktree).and_then(|git|
is_ancestor_of_in(&git, "HEAD", target_branch))`, keeping the three match arms; a pin failure
lands in the existing "cannot verify the worktree HEAD" refusal.
Command: `cargo test --lib orchestrator::merge_lifecycle::`

## X6: `loom/src/verify/wiring_detection.rs`

`collect_added_source_files` is ledgered at exactly 56 lines: keep 56. Replace the five-line
`run_git(...)` call and its `.with_context(...)?` with:

    // Pinned to the worktree's registered git directory: a process outside
    // the stage sandbox never follows the worktree's own `.git` file.
    let output = WorktreeGit::for_dir(worktree_path)?
        .run_read_only(&["diff", "--name-status", &format!("{}..HEAD", base_branch)])
        .with_context(|| format!("Failed to run git diff in {}", worktree_path.display()))?;

Swap the `use crate::git::runner::run_git;` line for `use crate::git::worktree::WorktreeGit;`.
Command: `cargo test --lib verify::wiring_detection::`

## X7: `loom/src/verify/duplicate_detection.rs`

The file is ledgered at exactly 436 lines: keep 436. In `collect_changed_source_files`, replace
the four-line `run_git(...)?;` call with:

    // Pinned to the worktree's registered git directory: a process outside
    // the stage sandbox never follows the worktree's own `.git` file.
    let output = WorktreeGit::for_dir(worktree_path)?
        .run_read_only(&["diff", "--name-only", &format!("{}..HEAD", base_branch)])?;

and swap the `run_git` import for `WorktreeGit` (confirm with a search that `run_git` has no
other use in the file first).
Command: `cargo test --lib verify::duplicate_detection:: && cargo test --test maintainability`

## X8: `loom/src/git/cleanup/worktree.rs`

`blocking_paths(worktree_path)` and `tracked_scaffold_paths(worktree_path)` run
`run_git_checked` in the worktree. Use `WorktreeGit::for_checkout(worktree_path).and_then(|git|
git.read_checked(<same args>))`, keeping `.unwrap_or_default()` / the empty-set fallback
(unit tests call `remove_worktree_scaffold` on plain directories, which `for_checkout` treats as
discovered git that fails, as today). `cleanup_worktree`'s `git worktree remove` runs in
`repo_root`: leave it.
Command: `cargo test --lib git::cleanup::`

## X9: `loom/src/git/cleanup/removal.rs`

- `require_clean_worktree(resources)`: the `status --porcelain=v1 --untracked-files=all
  --ignored=matching` call runs through `WorktreeGit::for_checkout(&resources.worktree_path)?`
  and `read_checked`, keeping `.context("Failed to verify worktree cleanliness")`.
- `require_merged_history`: the `"HEAD"` check in the worktree becomes a pinned ancestry check.
  Give `require_ancestor` a `&WorktreeGit` parameter and call it with
  `WorktreeGit::discovered(repo_root)` for the three `repo_root` checks and
  `WorktreeGit::pinned(repo_root, &resources.worktree_path)?` for the worktree HEAD, over
  `is_ancestor_of_in`. Error messages unchanged.
Command: `cargo test --lib git::cleanup::`

## X10: `loom/src/context/worktree_graph.rs`

`build_for_worktree(working_dir)` discovers everything with git run in `working_dir` (a
directory inside a stage worktree under an operator's host `loom stage complete` or `loom
verify`): `rev-parse --show-toplevel`, `rev-parse --path-format=absolute --git-common-dir`,
`rev-parse HEAD`, and `changed_paths` (`diff-index`, `ls-files --others`). Convert them:

- `let git = WorktreeGit::for_dir(working_dir)?;` at the top; `worktree` is
  `git.work_tree().to_path_buf()`; the common directory is
  `git.read_checked(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?` (a pinned
  handle answers with its `GIT_COMMON_DIR`, which is `R/.git`); `HEAD` through
  `git.read_checked`.
- `changed_paths(worktree, base_revision)` takes `&WorktreeGit` in place of the path and runs
  both commands through `read_checked`; every caller is in this file.
- `from_scratch(worktree, working_dir, extractors)` runs `ls-files --full-name ...` in
  `working_dir`, which limits the listing to that subtree. A pinned handle runs in the work tree
  instead, so pass the subtree as a pathspec: append `--` and `working_dir`'s path relative to
  the work tree (nothing when they are equal). Build the handle there with `for_dir(working_dir)`.
- Keep the doc comment on `build_for_worktree` true: it discovers the worktree and base through
  git pinned to the worktree's registered git directory.
- The file is not in the maintainability ledger; keep every function under 50 lines.
Command: `cargo test --lib context::worktree_graph`
