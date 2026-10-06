# Common brief: PLAN-open-issues-19-24

Every worker of every stage in `doc/plans/PLAN-open-issues-19-24.md` reads this file first, then
its own brief. The plan's "Decisions this plan settles" section is binding; where this file and
the plan's YAML differ, the YAML wins and you report the difference.

## Worker rules

- You own exactly the files your brief's "Files owned" list names. Read anything; write nothing
  else. A needed edit outside your list is reported to the orchestrator, not made.
- You are a leaf: never spawn subagents. Never run `git` write commands, `loom stage complete`,
  or `loom knowledge`. Record mistakes, decisions and surprises with `loom memory note` /
  `loom memory decision` as they happen; a knowledge claim the tree contradicts is
  `loom memory note "stale-knowledge: <file>#<heading> claims X; the tree does Y (file:line)"`.
- The crate does not compile mid-wave: other workers write the items you call. Write against the
  pinned interfaces below exactly. Run at most the one check your brief names, once; never
  `cargo fmt` (the orchestrator formats once after the wave).
- Paths: briefs give repository-relative paths (`loom/src/...`). Stage YAML uses
  package-relative ones (`src/...`) because every code stage has `working_dir: "loom"`.
- Limits: a file you touch stays at or under 400 lines and a function at or under 50 lines;
  past that, split into a sibling module. The maintainability ledger
  (`loom/maintainability-baseline.txt`) is the orchestrator's, never yours: report each ledgered
  unit you shrank.
- Tests: existing assertion lines are never edited (a test-integrity event); add new tests or
  new lines. A test file you move keeps its assertion lines verbatim. Tests never spawn the
  real loom binary except through `helpers::loom_cmd()`, never start a detached process that
  outlives the test (`mistakes/detached-spawn-in-tests.md`), never touch the live `.loom/work`
  or the real `HOME` (`mistakes/live-state-pollution.md`), and guard any Unix-socket bind with
  `crate::process::sandbox_probe::skip_unless(...)` as `loom/src/daemon/rpc.rs` tests do. Git in
  tests runs with `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` pointed at missing files and
  `GIT_CONFIG_NOSYSTEM=1`, `user.name`/`user.email` set locally (pattern:
  `loom/src/verify/impact_tests_tests.rs`). Env-mutating tests are `#[serial]`.
- Frozen contracts: the stage's `tests/<stage>_contracts.rs` is frozen before you start. Never
  edit it; make it pass. `loom stage contracts show <stage-id>` prints it.
- No AI attribution anywhere. Every fenced code block in Markdown names its language. No
  backwards-compatibility shims or migration code: the project is unreleased.
- Report: files changed, the one check you ran and its result, anything surprising or
  unresolved, every pinned interface the code made you deviate from (with why).

## Pinned interfaces: platform-portability

- **Socket (W2 writes `loom/src/daemon/socket.rs`; W1 declares it).** W1's
  `loom/src/daemon/mod.rs` gets `mod socket;` and
  `pub use socket::{socket_path, socket_path_fits, socket_path_problem, SOCKET_FILE, SUN_PATH_MAX};`.
  - `pub const SOCKET_FILE: &str = "orchestrator.sock";`
  - `pub const SUN_PATH_MAX: usize = 104;` (moved from
    `loom/src/daemon/server/lifecycle/socket_limit.rs`, which W1 deletes)
  - `pub fn socket_path_fits(path: &Path) -> bool` (byte length strictly below `SUN_PATH_MAX`)
  - `pub fn socket_path(work_dir: &Path) -> PathBuf` (canonicalized `work_dir` joined with
    `SOCKET_FILE`; on canonicalisation failure, the given spelling joined with it)
  - `pub fn socket_path_problem(work_dir: &Path) -> Option<String>` (None when
    `socket_path(work_dir)` fits; else a message with the byte count, `104` and the path, and
    the advice to move the repository to a path of at most 74 bytes)
  - The daemon's own bind keeps `work_dir.join(SOCKET_FILE)` (the daemon's work root is already
    real); every client uses `socket_path`.
- **Launch (W1).** `loom/src/daemon/server/launch.rs`:
  `pub struct ReadyTiming { pub deadline: Duration, pub grace: Duration }`,
  `pub fn await_ready(child: &mut Child, reader: std::io::PipeReader, log_path: &Path, timing: ReadyTiming) -> anyhow::Result<()>`,
  `pub(crate) fn spawn_daemon(...) -> anyhow::Result<()>` (W1 settles its parameters), called
  from `DaemonServer::start` as `launch::spawn_daemon(`. Re-exported:
  `loom::daemon::{await_ready, ReadyTiming}`. Readiness bytes: `0x01` ready, `0x02` output now
  in `orchestrator.log`. Deadline 10 s, grace 1 s in production.
- **Daemon-child argv (W1; W2's repair test pins it).** `<loom> run --daemon-child <ABS_WORK_ROOT>`
  followed by the run's own config flags. The flag is `#[arg(long, hide = true)]` and conflicts
  with `--foreground`. `loom/src/commands/run/mod.rs` dispatches it as
  `daemon_child::execute(`. `is_loom_run_cmdline` (`loom/src/commands/repair/daemon_checks.rs`)
  must return true for `/usr/local/bin/loom run --daemon-child /repo/.loom/work`.
- **Boot ID (W5 writes `loom/src/process/boot_id.rs`; the orchestrator adds `pub mod boot_id;`
  to `loom/src/process/mod.rs` before the wave).**
  - `pub const BOOT_ID_ENV: &str = "LOOM_BOOT_ID";`
  - `pub fn resolve_boot_id(env_value: Option<&str>, os_boot_id: impl FnOnce() -> anyhow::Result<String>) -> anyhow::Result<String>`
  - `pub fn os_boot_id() -> anyhow::Result<String>` (Linux `/proc/sys/kernel/random/boot_id`;
    macOS `sysctl -n kern.bootsessionuuid`; the bodies move here from
    `SystemBootClock::boot_id` in `loom/src/commands/subagents/wait/lease.rs`, which W4 then
    deletes)
  - `pub fn current_boot_id() -> anyhow::Result<String>` =
    `resolve_boot_id(std::env::var(BOOT_ID_ENV).ok().as_deref(), os_boot_id)`
  - W4 renders `LOOM_BOOT_ID` into the session wrapper from
    `WrapperHostEnv::boot_id: Option<String>`, filled in `launch/host.rs` with
    `crate::process::boot_id::os_boot_id().ok()`.

## Pinned interfaces: session-auth-and-stalls

- **Environment (E1, `loom/src/process/environment.rs`, re-exported from
  `loom/src/process/mod.rs`).** `pub const AGENT_SESSION_ENV_NAMES: &[&str]` (the wrapper's
  forwarding names, `USER` and `LOGNAME` included; `HOME` and `PATH` are handled separately);
  `pub fn agent_session_environment_from<I, K, V>(source: I) -> Vec<(OsString, OsString)>`;
  `pub fn apply_stage_environment_from<I, K, V>(command: &mut Command, source: I)` (today
  private; made pub). `STAGE_HOST_ENV_ALLOWLIST` gains `USER` and `LOGNAME`.
- **Auth probe (E1, `loom/src/claude/auth.rs`, declared `pub mod auth;` in
  `loom/src/claude.rs`).** `#[derive(Debug, Clone, PartialEq, Eq)] pub enum AuthProbe {
  LoggedIn { method: String }, NotLoggedIn, Unknown(String) }`;
  `pub fn parse_auth_status(stdout: &str, exit_success: bool) -> AuthProbe`;
  `pub fn stage_auth_status(claude_path: &Path) -> AuthProbe` (runs
  `<claude_path> auth status --json` with `env_clear()` plus
  `agent_session_environment_from(std::env::vars_os())`, bounded at 30 s through
  `crate::process` bounded-run helpers; never logs `email`, `orgId`, `orgName`). The claude
  binary is found with `crate::claude::find_claude_path()`.
- **Session tail (E3, `loom/src/orchestrator/terminal/session_tail.rs`, re-exported from
  `loom/src/orchestrator/terminal/mod.rs`).** `pub fn session_tail(session:
  &crate::models::session::Session, work_dir: &Path, lines: usize) -> Option<String>`: the last
  `lines` non-empty lines of the session's tmux pane (`capture-pane -p -J -S -<lines>` on the
  session's own `-L` socket, bounded by the tmux probe timeout), or for a native session the
  tail of its stderr log; `None` when nothing is readable. Output is control-character stripped.
- **Park reasons (E2).** Exhaustion: `stalled: session <id> silent <N>s (budget <B>s, last:
  <activity>) after <k> automatic recoveries; pane: "<last line>"`. Never worked, logged in:
  `session <id> never started work (no tool activity <N>s after start, budget <B>s); pane:
  "<last line>"`. Not logged in: `session <id> is not logged in to claude in the stage
  environment; run claude /login (as the operator) and then loom stage human-review <stage>
  --approve`. `Stage` has no `review_notes` field (the web view derives it from
  `review_reason`), so the pane tail is appended to `review_reason` after a blank line and
  `Last pane lines:`.
- **Never worked (E2, `loom/src/orchestrator/monitor/never_worked.rs`).** A Stage session whose
  own heartbeat shows `last_tool` None, `subagent` false and `context_tokens == 0` (SessionStart
  also fires on compaction and resume), or that has no heartbeat past `created_at` plus its
  non-zero budget. `MonitorEvent::SessionHung` and `HungReport` keep their shape; the handler
  recomputes the verdict through this predicate.

## Pinned interfaces: daemon-owned-commits

- **Commit core (C1, `loom/src/git/stage_commit.rs`, `pub mod stage_commit;` in
  `loom/src/git/mod.rs`).** As the plan's contract surface:
  `CommitRequest { message, expected_head, expected_tree }`,
  `CommitScope { StageBranch { stage_id }, Knowledge { target_branch, prefix }, Merge { stage_id } }`,
  `CommitRefusal` (Display, Debug), `pub fn commit_staged(repo: &Path, scope: &CommitScope,
  request: &CommitRequest) -> Result<String, CommitRefusal>` (a wrapper over
  `pub struct Committer { git, repo_root }` and its `commit_staged(&scope, &request)` method, which
  the daemon builds with a pinned `WorktreeGit`); `CommitRefusal::{Signing { detail }, Refused {
  reason }}`. The daemon handler (C2), not the core, takes the merge lock for a Knowledge scope and
  appends the target-guard attestation. A merge scope requires
  `MERGE_HEAD` and records it as the second parent, then clears the merge state
  (`git merge --quit`); any other scope refuses when `MERGE_HEAD` exists.
- **Signing (C1, `loom/src/git/signing.rs`, `pub mod signing;`).**
  `#[derive(Debug, Clone, Default)] pub struct SigningEnv { pub gnupghome: Option<OsString>,
  pub ssh_auth_sock: Option<OsString> }`; `impl SigningEnv { pub fn capture_from_process() ->
  Self }`; `pub fn install(env: SigningEnv)` (a `OnceLock`; a second call is ignored);
  `pub fn installed() -> &'static SigningEnv` (the default when nothing was installed);
  `pub fn signing_enabled(repo: &Path) -> anyhow::Result<bool>`
  (`git config --type=bool commit.gpgsign`); `pub fn probe(repo: &Path, env: &SigningEnv) ->
  anyhow::Result<()>` (an empty-tree `commit-tree -S` under a 30 s bound, through the new
  `crate::git::runner::run_git_with_env_within`). Test support shared with C2:
  `#[cfg(test)] pub(crate) mod tests` in `signing.rs` exposes `fake_signer(repo: &Path, fail: bool)`
  and `git_in(repo: &Path, args: &[&str]) -> String`. A signing failure blocks the stage through
  `handle_block_stage` (the inbox drain's Block arm), with the remedy and `loom stage retry`. The
  daemon child
  captures and installs at startup, before any thread, then removes `GNUPGHOME` and
  `SSH_AUTH_SOCK` from its own environment.
- **Relay (C2).** `RequestKind::Commit`, wire name `commit`, a control kind. Payload
  `pub struct CommitPayload { pub message: String, pub expected_head: String, pub
  expected_tree: String }` in `loom/src/relay/payload.rs`, exported from `loom::relay`, carried
  as JSON (`serde_json::to_value`). Matrix: Apply for `Stage`, `Knowledge`, `Merge`; Refuse for
  `Contract`, `Adjudication`, `BaseConflict`.
- **`loom stage commit` (U1, `loom/src/commands/stage/commit.rs`; C2 adds `pub mod commit;`
  and the CLI variant `StageCommands::Commit { stage_id: String, #[arg(short = 'm', long =
  "message")] message: String }`, dispatched as `commit::execute(stage_id, message)`).**
  `pub fn execute(stage_id: String, message: String) -> anyhow::Result<()>`: refuses a
  `stage_id` other than `LOOM_STAGE_ID` when that is set; validates the message (non-empty, at
  most 16 KiB, no NUL, no AI attribution); runs `git hook run --ignore-missing pre-commit` and
  `git hook run --ignore-missing commit-msg -- <message file>`; reads `git rev-parse HEAD` and
  `git write-tree` after the hooks; then relays `RequestKind::Commit` with `CommitPayload`
  through `RelayContext::check` and `RelayContext::emit` exactly as
  `loom/src/commands/stage/merge/relay.rs` does (operator and legacy modes call
  `crate::git::stage_commit::commit_staged` in-process instead). No `--amend`, `--author`,
  `--no-verify` or `--no-gpg-sign` flags exist.
- **`loom request status --wait` (U2, `loom/src/commands/request/status.rs`; C2 adds the
  `--wait <SECS>` arg to `RequestCommands::Status` in `loom/src/cli/types_ops.rs` and passes
  it in `loom/src/cli/dispatch.rs`).** `pub fn execute(id: String, session: Option<String>,
  wait_secs: Option<u64>) -> anyhow::Result<()>`: without `wait_secs`, today's behaviour; with
  it, poll the request's state every 500 ms until applied (exit 0, printing the commit id when
  the outcome carries one), refused (error naming the refusal), or the deadline (error
  "request <id> still pending after <N>s").
- **Doctrine sentence (C3; every surface uses this wording).** "Commit with
  `git add <specific-files>` then `loom stage commit <stage-id> -m "type(scope): description"`,
  and wait for it with `loom request status <id> --wait 120`; never run `git commit`." The
  knowledge prefix substitutes `doc/loom/knowledge/` for `<specific-files>` (an existing knowledge
  signal test forbids the generic placeholder there). The merge signal keeps the substring
  `git merge main` its tests pin, naming `git merge --no-commit --no-ff <target>` as the command.
