# G1: impact selection on the stage's own diff, runner readiness, fixture directories

Stage `completion-gates`, wave 1, tier sonnet. Read `../common.md` first.

## Why

At `loom stage complete`, a v2 standard stage runs the tests that reach what it changed
(`loom/src/verify/impact_tests.rs`). Three defects made Rust-only stages fail on `web/` vitest
files they never touched:

1. The changed set is `WorktreeGraph.changed`, which `context/worktree_graph.rs:57-80` computes
   as the diff from the nearest *published base layer* (or `HEAD` when none exists). A base
   older than the stage's branch point pulls in files other stages merged; with no base, only
   uncommitted files count, so a stage whose work is committed selects nothing.
2. A JS runner that cannot start (no `node_modules`, a refused registry, no `npx`) ends
   `Unparsed` with a non-zero exit, which `runs.rs:97-103` records as a failure.
3. `loom project detect` lists `loom/tests/fixtures/source/labeled/{rust,go}` as packages, so
   completion ran `cargo test` inside a labelled corpus (it wrote an untracked `Cargo.lock`).

You own: `loom/src/verify/impact_tests.rs`, `loom/src/verify/impact_tests/runs.rs`,
`loom/src/verify/impact_tests_tests.rs`, `loom/src/verify/review/fingerprint.rs`,
`loom/src/commands/stage/complete_verification_v2.rs`, `loom/src/skills/project/scan.rs`,
`loom/src/skills/project/tests.rs`.

Do not touch `loom/src/context/worktree_graph.rs`: `build_for_worktree` also serves reachable
checks (`verify/goal_backward/mod.rs:95`) and integration-verify's re-verification
(`complete_verification_v2.rs:97`), and `WorktreeGraph.changed` must keep meaning "differs from
the base layer the graph was built on" because `layered_graph` re-extracts exactly those
files. Only `impact_tests.rs:78` and one test assertion (`impact_tests_tests.rs:110`) read
`changed`.

## 1. One helper for "changed since the merge base"

The test-integrity gate and the review gate already diff against the stage's merge base:
`verify/review/fingerprint.rs::compute_local` (about lines 120-160) runs
`git merge-base HEAD <target_branch>`, lists `changed_names(repo, &base, None)` plus
`ls-files --others --exclude-standard -z` less `is_tool_artifact`, skips
`is_worktree_scaffold_path`, then hashes each file.

Extract the path listing into:

    /// The merge base of `HEAD` and `target_branch`, and every worktree-relative path changed
    /// since it: committed on the branch, changed in the index or working tree, or untracked
    /// and not ignored; worktree scaffolding and untracked sandbox artifacts left out.
    pub(crate) fn changes_since_merge_base(
        repo: &WorktreeGit,
        target_branch: &str,
    ) -> Result<(String, BTreeSet<String>)>

`compute_local` calls it and hashes the paths exactly as before (same entries, same order, so
`fingerprint_from` produces the same value). Every existing test in
`verify/review/fingerprint_tests.rs` must stay green without edits.

## 2. Impact selection uses it

In `impact_tests.rs`:

- Add the public function the contract calls:

      /// Worktree-relative paths the stage changed since `git merge-base HEAD <target_branch>`,
      /// sorted: committed, staged, unstaged and untracked-not-ignored.
      pub fn stage_changes(worktree: &Path, target_branch: &str) -> Result<Vec<PathBuf>>

  `worktree` is the worktree root. Use `fingerprint::local_git(worktree)?` (it pins git to a
  stage's registered git directory and falls back to discovery elsewhere) and the helper above;
  map the `BTreeSet<String>` to `PathBuf`s.
- `pub fn run(stage, working_dir, criteria_config, target_branch: &str)`: resolve the worktree
  root with `crate::git::runner::run_git_checked(&["rev-parse", "--show-toplevel"], working_dir)`,
  compute `stage_changes(&root, target_branch)`, and pass it to `run_with`.
- `fn run_with(stage, working_dir, graph, changed: &[PathBuf], runner)` passes `changed`
  (not `graph.changed`) to `reached_test_nodes`.
- Update the module doc comment (lines 1-11): the walk starts from every node in a file the
  stage changed since its merge base; the graph itself is still layered on the nearest
  published base.
- In `impact_tests_tests.rs`, `select_after_changing_add` passes `&graph.changed` as the new
  argument (that test changes one uncommitted file on `main`, so both sets are `src/lib.rs`).
  Its assertion lines stay as they are.

`commands/stage/complete_verification_v2.rs::run_impact_tests` takes `target_branch` from
`run_v2` (already a parameter there) and passes it on. Update its doc comment: "A test that
cannot be selected or run is a note, never a failure" stays true.

## 3. A runner that cannot start is a note

In `impact_tests/runs.rs`:

- Before `select_command` in `run_group`, check readiness: when
  `adapter.language() == "javascript"` (exactly the vitest, jest, mocha, bun-test and node-test
  adapters; every other adapter returns another language) and no directory named
  `node_modules` exists in the package directory or any ancestor up to and including the
  checkout root (`self.root`), insert the note

      `<package>` has no node_modules in this worktree (a plan `provision` entry installs it); the full suite runs in integration-verify

  and return without running anything. `<package>` is the package path relative to the
  checkout root as `group_targets` keys it (`web`), and `.` for the root package. Put the
  lookup in `fn missing_node_modules(package_dir: &Path, root: &Path) -> bool`; it never walks
  above `root`. A symlinked `node_modules` counts (use `Path::is_dir`, which follows it).
- In `judge`, before `classify`: a run whose `exit_code` is `Some(127)` (the shell's "command
  not found") inserts the note

      `<command>` could not start (exit 127); the full suite runs in integration-verify

  and records nothing else, whatever the adapter parsed. Every other outcome keeps its
  classification.

Unit tests in `impact_tests_tests.rs` (the `Recorder` there records commands instead of
running them; add a second recorder that answers exit 127 with empty output):

- `a_js_package_without_node_modules_is_a_note`: a repo whose `web/package.json` declares
  vitest and whose `web/src/a.test.ts` holds a named function; `run_with` with changed
  `[web/src/a.test.ts]` records no command and returns the note.
- `a_js_package_with_node_modules_in_an_ancestor_runs`: the same with `node_modules/` at the
  repository root; the recorder receives a `vitest run` command.
- `exit_127_is_a_note` (exact name; acceptance runs it): the 127 recorder yields that note and
  an empty `ran`, and `run_with` returns `Ok`.

A TypeScript test file needs at least one declaration (a named function) for the extractor
to give it a node; a file holding only a `test(...)` call has none and is never reached.

## 4. Fixture directories hold no packages

`skills/project/scan.rs` walks the checkout breadth-first and skips `SKIP_DIRS` by name.
Add `"fixtures"` to `SKIP_DIRS` with a comment: fixture trees carry manifests (the labelled
Rust and Go corpora) that are test data, not packages. Skipping the directory leaves out every
package whose path has a component named `fixtures`; the scan root itself is never compared.
Update the module or `SKIP_DIRS` doc so it says so.

Callers checked, for your report: `verify/contracts/mod.rs::detected_runner` (a contract's
runner), `verify/impact_tests.rs`, `skills/recommend.rs` (skill routing),
`commands/hook/project_types.rs`, `commands/project.rs` (`loom project detect`). All want
fixture manifests left out. No eval tooling calls detection.

Test in `skills/project/tests.rs`: `packages_under_a_fixtures_directory_are_not_detected`
(root `Cargo.toml`, `tests/fixtures/labeled/rust/Cargo.toml`, `fixtures/app/package.json`,
`tests/fixtures-extra/pkg/Cargo.toml`; only the root and `tests/fixtures-extra/pkg` remain).

## Check

None. G2 changes `DisputeKind` in parallel and the crate compiles again only after G3, in
wave 2; the main agent builds and tests then. Your code must satisfy the stage's contracts
`impact-selection-uses-the-stage-merge-base`, `js-runner-without-node-modules-is-a-note` and
`fixture-directories-hold-no-packages` (their scenarios are in the plan's YAML).
