# A: pinned host git, the commondir guard, pinned branch helpers, in-progress merges

Stage `host-git-integrity`, wave 1, tier opus. Read `../common.md` first (D2 and D7).

## Why

Loom's daemon, its monitor and an operator's `loom` CLI run git against a stage worktree `T`
outside every sandbox. Most of those calls let git discover its directory through `T/.git`, a
file the agent can rewrite. A rewritten pointer names a directory whose `config` defines a
`filter.<name>.clean` driver, and the host `git status`/`git diff` that re-hashes a stat-dirty
file runs it (`verify/contracts/test_support.rs::plant_foreign_git_dir` builds exactly that
fixture). Since `2908339a` the runner prepends `-c core.fsmonitor=false`
(`git/runner.rs::NO_HOOKS_ARGS`), so fsmonitor is closed; filters, `core.worktree` and forged
refs are not.

A second redirect was confirmed this session on git 2.53: a `commondir` file planted in the
MAIN repository's git directory `R/.git/commondir` makes every discovery-based git call in `R`
take its config, objects and refs from the named directory. With a foreign config defining a
clean filter and a same-length rewrite of a tracked file, `git -c core.hooksPath=/dev/null -c
core.fsmonitor=false status` in `R` ran the filter. `GIT_DIR=R/.git` alone does not stop it;
`GIT_COMMON_DIR=R/.git` does. Checkout-rooted sessions (knowledge, merge resolution) can write
`R/.git`, so they can plant it.

You build the pinned interface every other worker converts call sites to, and close the
`commondir` redirect for loom's own git.

## Files you own (relative to `loom/`)

`src/git/worktree/pinned.rs`, `src/git/worktree/pinned_tests.rs`, `src/git/runner.rs`,
`src/git/runner/pinned.rs`, `src/git/runner/pinned/tests.rs`, `src/git/branch/status.rs`,
`src/git/branch/status_tests.rs`, `src/git/branch/ancestry.rs`, `src/git/branch/mod.rs`,
`src/git/merge/in_progress.rs`.

Not yours: `src/git/mod.rs`, `src/git/merge/mod.rs` (worker B), `src/git/cleanup/**` (codex
units X8, X9). The codex units in wave 2 call exactly the signatures below; do not rename them.

## 1. `WorktreeGit` additions (`src/git/worktree/pinned.rs`)

Anchors (advisory, read at `3fc28031`, unchanged at `2908339a`): `WorktreeGit` :36,
`pinned` :66, `pinned_in_project_of` :83, `run` :97, `registered_admin_dir` :122.

```rust
/// Global options for a read that never writes the index back.
pub const READ_ONLY_ARGS: [&str; 1] = ["--no-optional-locks"];

impl WorktreeGit {
    /// Git for `checkout` as a process outside every stage sandbox runs it:
    /// pinned (`Self::pinned`) when `checkout`, canonicalized, is a stage
    /// worktree `<repo>/.worktrees/<name>` of a repository whose `.git` is a
    /// directory (not a symlink); as discovered for any other directory, the
    /// main checkout included. A stage-shaped checkout that cannot be pinned
    /// is an error, never discovered git.
    pub fn for_checkout(checkout: &Path) -> Result<Self>;

    /// `Self::for_checkout` for the checkout that holds `dir`: the innermost
    /// ancestor of `dir` (itself included) that is a stage worktree is pinned,
    /// its work tree that ancestor; with none, git is discovered at `dir`.
    /// A `.git` an agent planted below a stage worktree is never followed.
    pub fn for_dir(dir: &Path) -> Result<Self>;

    /// `git <args>` behind `READ_ONLY_ARGS`, through `Self::run`.
    pub fn run_read_only(&self, args: &[&str]) -> Result<Output>;

    /// `Self::run_read_only`, failing on a non-zero exit with the runner's
    /// error format; the trimmed stdout otherwise.
    pub fn read_checked(&self, args: &[&str]) -> Result<String>;

    /// The administrative directory a pinned handle runs with; `None` for a
    /// discovered one.
    pub fn git_dir(&self) -> Option<&Path>;
}
```

- One private helper decides stage shape: `fn stage_worktree_repo(path: &Path) -> Option<&Path>`
  returns `path.parent().parent()` when `path.parent()`'s file name is `.worktrees` and
  `symlink_metadata(<repo>/.git)` is a directory. `for_checkout` applies it to the checkout,
  `for_dir` to each of `dir.ancestors()` in order (innermost first).
- `read_checked` reuses the runner's success check: make `git/runner.rs::stdout_of_success`
  `pub(crate)` and call it; do not copy its message format.
- Update the module doc: name `for_checkout`/`for_dir` as the entry points for host code that
  has only a path, and `pinned`/`pinned_in_project_of` for code that knows the repository.

Known imprecision, document it on `for_dir` in one sentence: a repository that itself lives
inside an outer stage worktree (`/x/.worktrees/o/repo`) and is reached through `for_dir` on a
subdirectory is pinned to the outer worktree. `for_checkout(repo)` is exact for it. Callers
that hold the checkout root use `for_checkout`.

## 2. The `commondir` guard (`src/git/runner.rs`, `src/git/runner/pinned.rs`)

At `2908339a` the runner has `run_git_with_env` (discovery unless the caller's env pins) and
`runner/pinned.rs::run_git_pinned_within` (drops `GIT_DIR`, `GIT_WORK_TREE`,
`GIT_INDEX_FILE`, `GIT_COMMON_DIR` from the environment, then discovers). Note:
`run_git_pinned` is NOT D2 pinning; it still discovers through `.git`. Never use it for a stage
worktree.

Add one private function in `runner.rs`, called by `run_git_with_env` before it spawns and by
`run_git_pinned_within` before it spawns:

```rust
/// Refuses discovery-based git whose nearest `.git` at or above `repo_root`
/// is a directory holding `commondir`: git would take another repository's
/// configuration, objects and refs from the path it names. A main
/// repository's git directory never holds one (only `worktrees/<name>/`
/// does), and a session that can write the main git directory can plant it.
/// Skipped when `env` sets `GIT_DIR` or `GIT_COMMON_DIR`: the caller pinned.
fn refuse_redirected_git_dir(repo_root: &Path, env: &[(&str, &OsStr)]) -> Result<()>
```

Walk `repo_root.canonicalize()` ancestors (a path that does not resolve: return `Ok`, git will
fail on its own); at the first ancestor with a `.git` entry (`symlink_metadata`): a file means
`Ok`; a directory holding a `commondir` entry means `bail!` with a message containing
`commondir` and the path; otherwise `Ok`. Stats only, no git.

## 2b. Replace objects and submodules

- Every git command the runner starts carries `GIT_NO_REPLACE_OBJECTS=1`: set it in
  `runner.rs::git_command`, which both `run_git_with_env` and `runner/pinned.rs::pinned_command`
  build on, so it holds for discovered, env-pinned and `WorktreeGit` runs alike. A checkout
  session can write `R/.git/refs/replace/*` mid-session, and the daemon's own merges and
  ancestry checks in `R` would otherwise read the replacement object instead of the real one.
  Document it beside `NO_HOOKS_ARGS`. Before relying on it, run `rg -n 'replace' src --type
  rust` and confirm no loom code path depends on replace refs being honoured; report any.
- A PINNED `WorktreeGit::run` (a handle with a pin, whichever constructor made it) also
  prepends `-c diff.ignoreSubmodules=all -c submodule.recurse=false -c
  status.submoduleSummary=false`, as one constant `PINNED_WORKTREE_ARGS` beside
  `READ_ONLY_ARGS`. A tracked gitlink in a stage worktree otherwise makes host `git status` or
  `git diff` recurse into `<worktree>/<path>/.git`, a directory the agent wrote, and run its
  configuration. A discovered handle (git in `R`) does not get them.

## 3. Pinned branch helpers (`src/git/branch/status.rs`, `ancestry.rs`, `mod.rs`)

Add, and re-export from `branch/mod.rs`:

```rust
pub fn has_uncommitted_changes_in(git: &WorktreeGit) -> Result<bool>;
pub fn list_working_tree_changes_in(git: &WorktreeGit) -> Result<Vec<String>>;
pub fn commits_ahead_of_in(git: &WorktreeGit, branch: &str, base: &str) -> Result<usize>;
pub fn is_ancestor_of_in(git: &WorktreeGit, commit_sha: &str, branch: &str) -> Result<bool>;
```

Move each body into the `_in` function (status reads through `run_read_only`;
`working_tree_changes` takes `git.work_tree()` for `is_tool_artifact`; `branch_exists_ref`
takes the handle). The existing `&Path` functions become one-line wrappers over
`WorktreeGit::discovered(repo_root)`, so every existing caller and test keeps its behaviour.
Pattern to mirror: `verify/review/fingerprint.rs::compute_local`, which takes a `WorktreeGit`.
Do not copy its fingerprint logic.

## 4. In-progress merges (`src/git/merge/in_progress.rs`)

- `git_dir_for_repo_path(repo_path)`: a `.git` directory is returned as today. A `.git` FILE:
  `WorktreeGit::for_checkout(repo_path)?`; when `git_dir()` is `Some`, return it (the registered
  admin directory, whatever the file says); when `None` (a checkout loom did not create), parse
  the file as today.
- `unmerged_paths(repo_path)`: `WorktreeGit::for_checkout(repo_path)?.read_checked(&["diff",
  "--name-only", "--diff-filter=U"])`.
- Signatures of every `pub` function stay as they are (`commands/stage/complete.rs` and
  `commands/stage/merge/preflight.rs` call them and are not yours).

## Tests to add

In `pinned_tests.rs` (reuse `contract_worktree` and `plant_foreign_git_dir` from
`verify/contracts/test_support.rs`; positive control last, as the existing tests do):

- `a_stage_checkout_is_pinned_and_the_main_checkout_is_discovered`: `for_checkout(worktree)`
  has `git_dir() == Some(registered)`; `for_checkout(repo)` has `None`.
- `a_directory_inside_a_stage_worktree_is_pinned_past_a_nested_git_dir`: create
  `<worktree>/sub/.git` as a directory holding an `init`ed repository whose config defines the
  marker clean filter and `<worktree>/sub/.gitattributes` `* filter=evil` plus a stat-dirty
  tracked file of that nested repo; `for_dir(<worktree>/sub)` has the registered `git_dir()` and
  `run_read_only(&["status", "--porcelain"])` leaves the marker absent.
- `a_read_only_read_follows_no_repointed_git_file`: `plant_foreign_git_dir`, then
  `for_checkout(worktree)?.run_read_only(status)`; marker absent; then discovered status
  creates it.
- `a_stage_shaped_directory_the_repository_does_not_register_is_refused`:
  `<repo>/.worktrees/stranger` as a plain directory; `for_checkout` is `Err` containing
  `is not a registered worktree`.

In `runner.rs` tests (reuse `isolated_git`/`isolated_git_ok`):

- `a_main_git_dir_holding_commondir_is_refused`: repository `R` with committed `README.md`
  (`one\n`); a bare clone `foreign.git` beside it with `core.bare=false` and
  `filter.evil.clean = "touch <marker>; cat"`; `R/.gitattributes` `* filter=evil`; rewrite
  `R/README.md` to `two\n` (same length, so git must re-hash it); write
  `R/.git/commondir` naming `foreign.git`. `run_git(&["status", "--porcelain"], &R)` is `Err`
  containing `commondir`; the marker is absent. Positive control last: plain `git status` in
  `R` creates the marker.

- `a_planted_replace_ref_is_ignored_by_loom_git`: repository with commits C1 and C2 on
  `main`; `git replace <C1> <C2>` with plain git; `run_git_checked(&["cat-file", "-p", C1],
  R)` prints C1's own content (its own tree line), while plain `git cat-file -p C1` (the
  positive control, last) prints C2's.

In `pinned_tests.rs` also:

- `a_pinned_status_does_not_recurse_into_a_gitlink`: in the stage worktree, record a gitlink
  `sub` (`git update-index --add --cacheinfo 160000,<oid>,sub` and a commit), make `<worktree>/
  sub` an `init`ed repository with one committed file, whose config defines the marker clean
  filter and whose `.gitattributes` applies it, and leave that file stat-dirty with a
  same-length rewrite; `for_checkout(worktree)?.run_read_only(&["status", "--porcelain"])`
  leaves the marker absent. Positive control last: plain `git status` in the worktree creates
  it. If plain git does not recurse in your fixture, find the git setting that makes it (and
  that the constant turns off) before writing the assertion; never ship a control that cannot
  fire.

In `status_tests.rs`: `pinned_status_lists_changes_through_the_registered_git_dir`
(`list_working_tree_changes_in(&WorktreeGit::for_checkout(worktree)?)` lists a file changed in
the worktree).

In `in_progress.rs` tests: `a_worktree_merge_head_is_read_from_its_registered_git_dir` (write a
`MERGE_HEAD` into the registered admin directory, repoint `T/.git` to a directory without one;
`detect_in_progress_merge_at_worktree(T)` finds the merge; a `MERGE_HEAD` written only into the
repointed directory is not reported).

Git in tests: `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` pointed at missing files,
`GIT_CONFIG_NOSYSTEM=1`, identity and `commit.gpgsign=false` via `-c`.

## Traps

- `WorktreeGit::pinned`'s own `common_dir(repo_root)` runs `git rev-parse --git-common-dir` in
  `R` through the runner, so the guard covers it: do not add a second check there.
- Never derive `R` from git run inside `T` (`rev-parse --show-toplevel` or
  `--git-common-dir` in `T` follows `T/.git`).
- `--no-optional-locks` is a global option: it goes before the subcommand, after the
  runner's `-c` pairs, which `run` already places.
- Keep `pinned.rs` under 400 lines; move the new tests to `pinned_tests.rs`, not inline.

## Check

Once, after your last edit: `cargo test --lib git::` (worker B writes other `git/` files in the
same wave; a compile error in a file you do not own is reported with its file:line, not
fixed).
