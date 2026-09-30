# P1: the `provision` field, its executor and the spawn gate

Stage `plan-environment`, wave 1, tier sonnet. Read `../common.md` first. P2 (wave 2) reads
the field you add; P3 writes the docs in parallel.

You own `loom/src/plan/schema/{types.rs, types_v2.rs, mod.rs, validation/v2_fields.rs,
tests/v2_tests.rs}`, `loom/src/orchestrator/{provision.rs (new), mod.rs}` and
`loom/src/orchestrator/core/{provision_gate.rs (new), provision_gate_tests.rs (new), mod.rs,
stage_executor.rs}`.

## Why

A worktree is cut from git, so it has no `node_modules`. JS tests cannot run there, and a stage
session sandbox may not reach the npm registry. The settled design: a plan-level

    loom:
      provision:
        - working_dir: web
          command: bun install --frozen-lockfile

that the daemon runs on the host, in the stage's worktree, before each stage session spawns. A
failure blocks the stage with the error as its reason. Commands must be idempotent: a retry, a
handoff successor or an adjudication requeue runs them again.

## 1. Schema (`version: 2` only)

- `plan/schema/types_v2.rs`:

      /// A command the daemon runs on the host, in `<worktree>/<working_dir>`, before each
      /// stage session spawns (plan `version: 2`). It must be idempotent: every spawn of
      /// every stage runs it again.
      #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
      #[serde(deny_unknown_fields)]
      pub struct ProvisionEntry {
          /// Directory to run in, relative to the repository root; no `..`.
          pub working_dir: String,
          /// Shell command, run with `sh -c`.
          pub command: String,
      }

- `plan/schema/types.rs` is ledgered at exactly 416 lines and must stay there (net-zero):
  - line 5 becomes `use super::types_v2::{deserialize_reasoning_effort, ProvisionEntry};`;
  - add to `LoomConfig`, after `ratchet_files`, exactly three lines:

        /// Commands the daemon runs on the host in each stage worktree before a session spawns.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub provision: Vec<ProvisionEntry>,

  - offset them by replacing the four-line comment above `pub use
    crate::models::stage::PermissionMode;` (lines 7-10) with the one line
    `/// Claude Code permission mode; defined in [`crate::models::stage::PermissionMode`].`
  - `wc -l` must still say 416. `cargo test --test maintainability` is in acceptance.
- `plan/schema/mod.rs`: `pub use types_v2::ProvisionEntry;` so it is
  `loom::plan::schema::ProvisionEntry`.
- `plan/schema/validation/v2_fields.rs` (`validate()` calls `push_v2_field_errors`; do not
  touch `validation.rs`, whose `validate` is ledgered at 357 lines):
  - `push_v1_uses`: a non-empty `provision` on a v1 plan gives "`provision` requires
    `version: 2`", as `ratchet_files` does.
  - `push_v2_rules`: for entry `#n` (1-based), reuse `path_problem` for `working_dir`
    ("provision entry #n working_dir '<dir>' must be a relative path" / "cannot contain a `..`
    component"); an empty or whitespace `working_dir` gives "provision entry #n working_dir
    must not be empty" (use "." for the repository root); an empty command gives "provision
    entry #n has an empty command". Each message starts "provision".
- Tests in `plan/schema/tests/v2_tests.rs`: a v1 plan with provision; a v2 plan with `..`, an
  absolute path, an empty dir and an empty command; a valid entry passing. Existing assertion
  lines stay.

## 2. The executor: `loom/src/orchestrator/provision.rs`

Declare `pub mod provision;` in `orchestrator/mod.rs` (alphabetical among the `pub mod`s).

    /// How long one provision command may run.
    pub const PROVISION_TIMEOUT: Duration = Duration::from_secs(600);

    /// Run the plan's provision entries in `worktree`, in order, stopping at the first
    /// failure. `Err` is the stage's block reason.
    pub fn run_provision(entries: &[ProvisionEntry], worktree: &Path) -> Result<(), String>

For each entry:

1. `dir = worktree.join(&entry.working_dir)`. Canonicalize `worktree` and `dir`; a missing
   directory, or a canonical `dir` that does not start with the canonical worktree (a symlink
   pointing out), is an `Err` and nothing runs.
2. Run it the way `before_stage` checks run (`verify/before_after.rs::run_before_stage_checks`
   → `verify/goal_backward/truths.rs::verify_truth_checks`): on the host, in the daemon's
   process, through `crate::verify::criteria::run_spec_with_timeout(&CommandSpec::shell(..),
   Some(&dir), PROVISION_TIMEOUT, CommandConfinement::Confined)`. `Confined` keeps `HOME` and
   `PATH` (`daemon/server/environment.rs::HOST_ENV_ALLOWLIST`), so `bun` finds its cache.
3. A spawn error, a timeout or a non-zero exit is an `Err`.

The reason is exactly

    provision `<command>` in `<working_dir>` failed: <detail>

with `<working_dir>` as the plan wrote it, and `<detail>`: the last 10 non-blank lines of
stderr joined with `\n`; when stderr is blank, the last 10 non-blank lines of stdout; when both
are blank, `exit <code>` (or `killed by a signal`). A timeout's detail is `timed out after
600 s`; a missing directory's is `the directory does not exist in the worktree`; an escape's is
`the directory resolves outside the worktree`.

Unit tests (inline module, or `provision_tests.rs` via `#[path]`): runs in the working dir,
stops at the first failure, the stderr tail, the stdout fallback, a missing dir, a symlink
escape. Keep the file under 400 lines and each function under 50.

## 3. The spawn gate

`orchestrator/core/stage_executor.rs` is ledgered (597 lines; `start_stage` 149;
`before_stage_gate_passed` 62). Keep every count unchanged:

- In `start_stage` (about lines 298-302) replace the two comment lines and the call
  `if !self.before_stage_gate_passed(&stage, &worktree.path, resolved.branch_name())? {` with
  two comment lines ("Provision the worktree, then run the before-stage checks. Either one
  failing blocks the stage.") and `if !self.pre_spawn_gates_passed(&stage, &worktree.path,
  resolved.branch_name())? {` (under 100 columns, one line).
- `fn before_stage_gate_passed` becomes `pub(super) fn before_stage_gate_passed` (same line
  count).

New `orchestrator/core/provision_gate.rs`, declared `mod provision_gate;` in
`orchestrator/core/mod.rs`:

    impl Orchestrator {
        /// Provision the stage's worktree, then run its `before_stage` gate. `Ok(false)` means
        /// the stage was blocked and must not spawn.
        pub(super) fn pre_spawn_gates_passed(&mut self, stage: &Stage, worktree_path: &Path,
            base_branch: &str) -> Result<bool>
    }

- Read the entries from the live plan: `crate::fs::resolve_source_path(&self.config.work_dir)?`;
  `None` means no entries; otherwise `crate::plan::parser::parse_plan(&path)?.metadata.loom.provision`.
  Reading the plan at every spawn lets an operator add an entry and run `loom stage retry`.
  A plan that cannot be read or parsed blocks the stage with the reason
  `provision: cannot read the plan: <error:#>`.
- No entries: go straight to `before_stage_gate_passed`.
- Otherwise print `Provisioning '<stage-id>' (<n> command(s))...`, call `run_provision`, and
  on `Err(reason)` block the stage and return `Ok(false)` without running the before-stage
  gate. Blocking mirrors `stage_executor.rs::persist_blocked_stage` (lines 18-34) and the
  before-stage failure path: in one `self.update_stage(..)`, `try_mark_blocked()`, set
  `failure_info = Some(FailureInfo { failure_type: FailureType::InfrastructureError,
  detected_at: Utc::now(), evidence: <the reason's lines> })` and `close_reason =
  Some(reason)`; then `self.graph.mark_status(id, StageStatus::Blocked)`. Setting
  `failure_info` keeps `loom status` from reading the block as an agent's
  (`InfrastructureError` is never auto-retried: `orchestrator/retry.rs:12-21`).
- The gate runs synchronously, like the before-stage gate, and only on the worktree spawn path
  (`start_stage` returns before it for knowledge stages). The contract-to-implementation handoff
  (`event_handler/contract_phase.rs::spawn_on_stage_worktree`) reuses the worktree the first
  spawn provisioned and does not run it.

Tests in `provision_gate_tests.rs` (`#[cfg(test)] #[path = "provision_gate_tests.rs"] mod
tests;` inside `provision_gate.rs`). Model the orchestrator setup on
`stage_executor_tests.rs::work_dir`/`orchestrator_for` (copy the few lines; that file is not
yours). Write `config.toml` with `[plan]\nsource_path = "<absolute plan path>"` (as
`tests/adjudication_e2e.rs::write_config` does) and a v2 plan with a provision entry.

- `a_failing_provision_blocks_the_stage_with_its_reason` (exact name; acceptance runs
  `orchestrator::core::provision_gate::tests::a_failing_provision_blocks_the_stage_with_its_reason`):
  an entry `exit 3` in `.`; `pre_spawn_gates_passed` returns `Ok(false)`; the stage file is
  Blocked, `close_reason` starts "provision `exit 3` in `.` failed", `failure_info` is
  `InfrastructureError`.
- `a_plan_without_provision_leaves_the_gate_to_before_stage`: `Ok(true)` and the stage is
  unchanged.
- `a_passing_provision_runs_in_the_worktree`: `touch provisioned` in `.` leaves the file in the
  worktree and returns `Ok(true)`.

## Contracts your code must satisfy

`provision-runs-in-its-working-dir`, `failed-provision-names-command-and-dir`,
`provision-refuses-a-symlinked-escape`, `provision-working-dir-cannot-escape` (scenarios in the
plan's YAML).

## Check

`cargo test --lib orchestrator::provision::`, once.
