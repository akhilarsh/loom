# C3: launch wiring (host facts, capsule merge, wrapper env, confined env, retirement)

Stage `capsule-policy`, wave 2, tier sonnet. Read `../common.md` first. C1 (session caches) and
C2 (capsule policy) finished in wave 1; you connect their functions to every spawn, to the
wrapper, to confined commands and to retirement, and you bring the native test fixtures along.
The crate compiles again only once your files match their changes.

## Files you own

- `loom/src/orchestrator/terminal/native/launch.rs`
- `loom/src/orchestrator/terminal/native/launch/host.rs`
- `loom/src/orchestrator/terminal/native/session_settings.rs`
- `loom/src/orchestrator/terminal/native/session_settings/contents.rs`
- `loom/src/orchestrator/terminal/native/wrapper/host_env.rs`
- `loom/src/orchestrator/terminal/native/tests_wrapper_env.rs`
- `loom/src/orchestrator/terminal/native/tests_launch_capsule.rs`
- `loom/src/orchestrator/terminal/native/tests_session_settings.rs`
- `loom/src/orchestrator/terminal/native/tests_capsule.rs`
- `loom/src/orchestrator/terminal/native/tests_capsule_contents.rs`
- `loom/src/orchestrator/terminal/native/tests_capsule_interpreters.rs`
- `loom/src/orchestrator/terminal/native/tests_confinement_e2e.rs`: ONLY the `LaunchHost`
  literal's one new field (section 1) and its `use` line, granted to this stage as a
  compile-only edit. The sandbox-canary stage owns the rest
  of that file; change no probe, no assertion, nothing else.
- `loom/src/process/environment.rs`
- `loom/src/orchestrator/core/inbox_drain/sweep.rs`
- `loom/src/orchestrator/core/inbox_drain/tests_sweep_cache.rs` (new)

Not yours: `wrapper.rs` (its `build_wrapper_script` already renders `WrapperHostEnv::render`, so
it needs no change), `orchestrator/core/inbox_drain.rs` (register your sweep tests from
`sweep.rs` with `#[path]`), every file under `sandbox/`.

## What wave 1 gives you (pinned signatures)

From C1, `crate::sandbox::session_cache`:

    pub const CODEX_HOME_DIR: &str = "codex-home";
    pub const SESSION_OWNER_MARKER: &str = ".loom-session";  // C's owner marker (session id)
    pub const SESSION_CACHE_VARIABLES: [&str; 18];            // variable names to forward
    pub const SESSION_GIT_CONFIG_ENV: [(&str, &str); 5];      // fixed GIT_CONFIG_* pairs
    pub struct RealCaches { pub cargo_home: PathBuf, ..., pub codex_home: PathBuf, ... }
    impl RealCaches { pub fn from_lookup(home: &Path, lookup: &dyn Fn(&str) -> Option<OsString>) -> Self; }
    pub struct Ecosystems { pub cargo: bool, pub bun: bool, pub npm: bool, pub pnpm: bool,
                            pub go: bool, pub uv: bool }       // Default, Copy
    impl Ecosystems { pub fn detect(dir: &Path) -> Self; }
    pub fn session_cache_root(work_dir: &Path) -> anyhow::Result<PathBuf>;   // cfg(test): <work_dir>/session-caches
    pub fn session_cache_key(stage_id: Option<&str>, kind: SessionType, session_id: &str)
        -> anyhow::Result<String>;                              // "<stage-id>-<kind>", or the session id
    pub fn session_cache_dir(root: &Path, key: &str) -> anyhow::Result<PathBuf>;
    pub fn prepare_session_cache(root: &Path, key: &str, session_id: &str, uid: u32,
                                 real: &RealCaches, used: Ecosystems, codex_licensed: bool)
        -> anyhow::Result<PathBuf>;                             // replaces any previous C, writes the marker
    pub fn session_cache_env(dir: &Path, real: &RealCaches, codex_licensed: bool) -> Vec<(String, String)>;
    pub fn release_session_cache(dir: &Path, session_id: &str) -> anyhow::Result<bool>;  // removes C only for its owner
    pub fn remove_session_cache(dir: &Path) -> anyhow::Result<()>;

`C` is `<root>/<stage-id>-<session-kind>`: every session of one stage and kind gets the same
path (so cargo does not recompile registry dependencies each session), and each spawn replaces
the previous session's content. The marker says which session a `C` belongs to now.

From C2, `crate::sandbox::session_fs`:

    pub struct SessionFsInputs<'a> { pub kind: SessionType, pub repo_root: &'a Path,
        pub worktree: Option<&'a Path>, pub git_common_dir: &'a Path, pub cache_dir: &'a Path,
        pub home: Option<&'a Path>, pub codex_home: &'a Path, pub codex_licensed: bool,
        pub credential_paths: &'a [PathBuf] }
    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    pub struct SessionFs { pub allow_write: Vec<String>, pub deny_write: Vec<String>,
        pub deny_read: Vec<String>, pub allow_read: Vec<String>, pub edit_deny: Vec<String> }
    pub fn session_filesystem(inputs: &SessionFsInputs<'_>) -> anyhow::Result<SessionFs>;
    pub fn resolve_git_common_dir(repo_root: &Path) -> anyhow::Result<PathBuf>;
    pub fn env_credential_paths(lookup: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf>;

C2 also removed `DenyInputs::plugin_entries`, `codex_plugin_entries`, `CODEX_PLUGIN_DATA_GRANT`
and `PACKAGE_MANAGER_CACHE_WRITE_PATHS`; `CODEX_SANDBOX_WRITE_PATHS` is now
`["~/.codex/plugin-data"]`. Read C2's and C1's reports if the main agent passes them; the files
themselves are the truth.

## 1. `launch/host.rs`: resolve the new facts once, in `from_env`

`LaunchHost` (`host.rs:30-43`) resolves every host fact in `from_env` and tests set fields
directly; keep that invariant. Add ONE field and one struct:

    /// The per-spawn facts a session's capsule and cache need beyond the host checks.
    pub(super) struct SessionFacts {
        /// The repository's git common directory, canonical (D1).
        pub(super) git_common_dir: PathBuf,
        /// Where session cache directories live (D3).
        pub(super) cache_root: PathBuf,
        /// The operator's real caches and codex home, from the daemon's environment.
        pub(super) real_caches: RealCaches,
        /// Credential locations the daemon's environment relocates (D4).
        pub(super) credential_paths: Vec<PathBuf>,
    }
    impl SessionFacts {
        pub(super) fn resolve(repo_root: &Path, cache_root: PathBuf, home: &Path,
                              lookup: &dyn Fn(&str) -> Option<OsString>) -> Result<Self>;
    }
    // LaunchHost gains: pub(super) session: SessionFacts,

`resolve` calls `resolve_git_common_dir(repo_root)?` (a repository that does not resolve fails
the spawn), `RealCaches::from_lookup(home, lookup)` and `env_credential_paths(lookup)`. In
`from_env` (`host.rs:49-65`): `SessionFacts::resolve(&repo_root, session_cache_root(work_dir)?,
home, &|name| std::env::var_os(name))?`, where a missing home fails with "cannot determine the
home directory; refusing to spawn". `cache_root` is a parameter so a test fixture can put it
outside the repository: a cache root under `<repo>/.loom` would be write-denied by the capsule's
own `.loom` deny.

In `writable_roots` (`host.rs:243-275`) push `session_cache_root(work_dir)` (when `Ok`) onto the
roots `session_writable_roots` returns, so `LOOM_BIN` and hook PATH entries under it are refused.
Do not change `WritableRootInputs`: the canary's e2e constructs it.

Two methods:

    /// Create this session's cache directory C at `<cache root>/<key>` (replacing the previous
    /// session's, 0700, owner marker written, seeded for the ecosystems `cwd` uses); with the
    /// codex lane, first make sure `~/.codex/plugin-data` exists, because the capsule grants it
    /// and the forwarder's `mkdir -p` cannot create it from inside.
    pub(super) fn prepare_cache(&self, key: &str, session_id: &str, cwd: &Path,
                                codex_licensed: bool) -> Result<PathBuf>;
    /// The wrapper's `LOOM_SCRATCH_DIR`, `LOOM_BIN`, `LOOM_HOOK_PATH` and the session's
    /// cache environment (`session_cache_env`).
    pub(super) fn wrapper_env(&self, dirs: &SessionDirs, codex_licensed: bool) -> WrapperHostEnv;

    pub(super) struct SessionDirs { pub(super) scratch: PathBuf, pub(super) cache: PathBuf }

`prepare_cache` calls `prepare_session_cache(&self.session.cache_root, key, session_id,
self.uid, &self.session.real_caches, Ecosystems::detect(cwd), codex_licensed)` with context
"cannot prepare the session cache directory; refusing to spawn". The plugin-data directory is created
under `self.home` (skip when `None`) with `DirBuilder::new().recursive(true).mode(0o700)`; a
failure there is a `tracing::warn!`, not a spawn failure (the forward reports its own error).

`host.rs` is 315 lines and these additions keep it under 400. Keep them in `host.rs`: the
plan's wiring checks look for `resolve_git_common_dir(`, `prepare_session_cache(`,
`session_cache_env(` and `Ecosystems::detect(` in that file.

## 2. `launch.rs`

`prepare_session_launch_with` (`launch.rs:298-343`): after `require_confined_host`, compute
`key = session_cache_key(Some(&stage.id), kind, &session.id)?` (the stage is already assigned
there) and build `SessionDirs { scratch: host.prepare_scratch(&session.id)?, cache:
host.prepare_cache(&key, &session.id, cwd, codex)? }` with
`codex = sandbox.implementers.includes_codex()`, pass `&dirs` to
`write_capsule`, and `host.wrapper_env(&dirs, codex)` to `create_session_wrapper_script`. The
function is 46 lines; extract a helper so it stays under 50. `write_capsule` (`launch.rs:256-279`)
takes `dirs: &SessionDirs` in place of `scratch_dir` (7 parameters, clippy's limit) and fills the
new `CapsuleRequest::session` (section 3) from `host.session` and `dirs.cache`.

## 3. `session_settings.rs`

- New, next to `CapsuleRequest`:

      /// The launch-resolved paths a capsule's session layer needs.
      pub(super) struct SessionPaths<'a> {
          pub git_common_dir: &'a Path,
          pub cache_dir: &'a Path,
          pub codex_home: &'a Path,
          pub credential_paths: &'a [PathBuf],
      }

  and `CapsuleRequest` gains `pub session: SessionPaths<'a>` (doc it).
- `write_session_capsule` (`:82-121`) builds the session layer after `let worktree = ...;` in a
  helper `fn session_layer(request, repo_root: &Path, worktree: Option<&Path>) ->
  Result<SessionFs>` that calls `session_filesystem(&SessionFsInputs { kind: request.kind,
  repo_root, worktree, git_common_dir: request.session.git_common_dir, cache_dir:
  request.session.cache_dir, home: request.surfaces.home(), codex_home:
  request.session.codex_home, codex_licensed: request.sandbox.implementers.includes_codex(),
  credential_paths: request.session.credential_paths })`, and passes `session_fs: &session_fs`
  to `CapsuleInputs`. Keep the function under 50 lines.
- `capsule_denies` (`:146-169`): drop the `plugin_entries` computation and field; the
  `codex_plugin_entries` import goes. Update both doc comments (the module doc at `:1-10` gains
  one sentence: the capsule also carries the session's own filesystem layer,
  `sandbox::session_fs`).

## 4. `session_settings/contents.rs`

`CapsuleInputs` (`:27-52`) gains `pub session_fs: &'a SessionFs` ("the session's own
filesystem layer: its cache grant, git and codex denies, credential and sibling-worktree read
denies"). In `capsule_settings` (`:62-91`), after the `denies` lines, merge with the existing
`extend_strings`: `allow_write` into `sandbox.filesystem.allowWrite`, `deny_write` into
`denyWrite`, `deny_read` into `denyRead`, `allow_read` into `sandbox.filesystem.allowRead` (a
new key; `extend_strings` writes nothing for an empty list, so a capsule without one has no
`allowRead` key), `edit_deny` into `permissions.deny`. Update the module doc and the
`capsule_settings` doc to name the session layer. The capsule still never carries `env`.

## 5. `wrapper/host_env.rs`

`WrapperHostEnv` (`:20-28`) gains `pub session_env: Vec<(String, String)>` (doc: "the session's
cache environment, `sandbox::session_cache::session_cache_env`, exported after the loom
variables"). `render` (`:33-60`) appends one continuation line per pair, rendered exactly like
the others (`escape(format!("{name}={value}").into())`). The default renders nothing, as
before. Update the module doc's first sentence to mention the cache environment.

## 6. `process/environment.rs`: confined commands

`loom stage complete` runs a stage's acceptance commands inside the session through
`spawn_confined`, which rebuilds the environment from `STAGE_HOST_ENV_ALLOWLIST`. Without the
session's cache variables those commands would fall back to the real caches, which are read-only
now, and fail with EROFS.

- `is_allowed` (`:99-103`) accepts a name in `STAGE_HOST_ENV_ALLOWLIST` OR in
  `SESSION_CACHE_VARIABLES`. Leave the existing list and its comments alone.
- `apply_stage_environment_from` (`:84-97`), after forwarding, sets every
  `SESSION_GIT_CONFIG_ENV` pair. The host's own `GIT_CONFIG_*` are never forwarded (they can
  carry a credential, for instance an `http.extraheader`); the fixed pair is the only git
  configuration a confined command gets from the environment.
- `apply_stage_environment_from` also sets `LOOM_ACCEPTANCE_STAGE_ID`,
  `LOOM_ACCEPTANCE_SESSION_ID` and `LOOM_ACCEPTANCE_WORK_DIR` from the source's
  `LOOM_STAGE_ID`, `LOOM_SESSION_ID` and `LOOM_WORK_DIR` when each is set and non-empty (one
  constant `ACCEPTANCE_CONTEXT_VARIABLES: [(&str, &str); 3]` pairing source and target names).
  The `LOOM_*` names themselves stay withheld, so a `loom` subcommand inside an acceptance command
  keeps its operator behaviour; the copies exist for the in-session canary (sandbox-canary stage),
  which must find its session's capsule when integration-verify's `loom stage complete` runs it.
  No loom code reads the `LOOM_ACCEPTANCE_*` names.
- Doc both on `STAGE_HOST_ENV_ALLOWLIST`: inside a session the forwarded cache variables point
  into the session's own cache directory, so an acceptance command builds from it; outside a
  session they carry the daemon's own values.

Tests (in the existing `mod tests`, exec'ing `/usr/bin/env` like the two tests there):
`session_cache_locations_survive_a_confined_run` (source `CARGO_HOME`, `npm_config_cache`,
`GOPROXY`, `XDG_CACHE_HOME` and a `GITHUB_TOKEN` canary: the four survive, the canary does not)
and `git_config_is_fixed_and_never_forwarded_from_the_host` (exact name; acceptance runs it:
source `GIT_CONFIG_COUNT=1`, `GIT_CONFIG_KEY_0=http.extraheader`, `GIT_CONFIG_VALUE_0=AUTHORIZATION:
basic secret-canary`; output holds `GIT_CONFIG_KEY_0=gc.auto` and neither `http.extraheader` nor
`secret-canary`), and `session_identity_reaches_acceptance_under_its_own_names` (exact name;
acceptance runs it: source `LOOM_STAGE_ID=s1`, `LOOM_SESSION_ID=sess-1`, `LOOM_WORK_DIR=<dir>`;
output holds `LOOM_ACCEPTANCE_STAGE_ID=s1`, `LOOM_ACCEPTANCE_SESSION_ID=sess-1` and
`LOOM_ACCEPTANCE_WORK_DIR=<dir>`, and no line starting `LOOM_STAGE_ID=` or `LOOM_SESSION_ID=`).

## 7. `inbox_drain/sweep.rs`: retirement removes C

Where retirement removes the scratch directory (`retire`, `sweep.rs:98-126`), also release the
session's cache directory. `retire` already has the parsed `record`:

    /// Remove this session's cache directory when it still owns it: the path is shared by
    /// every session of one stage and kind, so a later session may own it by now.
    fn release_cache(work_dir: &Path, record: &Session) -> Result<()>

(`session_cache_root(work_dir)`, `session_cache_key(record.stage_id.as_deref(),
record.session_type, &record.id)`, `session_cache_dir(&root, &key)`, then
`release_session_cache(&dir, &record.id)`; `Ok(false)` is success). Never call
`remove_session_cache` from retirement: a late retire of a crashed session must not delete its
successor's cache. In `retire`, after the scratch removal, push a failure into `failures` like
the others, and when this one failed skip `cleanup_session_settings` for this pass: the capsule
left on disk keeps the cheap early return (`:57-62`) from skipping the session, so the next tick
retries. Leave the early return itself unchanged (computing the key needs the parsed record). A
`C` that is never released is replaced by the next spawn of the same stage and kind. Update
`retire`'s doc comment. Register the tests at the end of `sweep.rs`:

    #[cfg(test)]
    #[path = "tests_sweep_cache.rs"]
    mod tests_cache;

`tests_sweep_cache.rs` mirrors `tests_sweep.rs` (`super::super::test_support::{fixture, ...}`,
`super::sweep_sessions`, `super::super::PassReport`):

Build each cache with C1's `prepare_session_cache` (key from `session_cache_key` for the
record, root from `session_cache_root(&fx.work_dir)`, a `RealCaches::from_lookup` over a
TempDir home, `Ecosystems::default()`), so the marker is the real one.

- `retirement_removes_the_session_cache_and_leaves_linked_real_files` (exact name; acceptance
  runs it): a Completed Knowledge record on a Completed stage and its prepared cache holding
  `cargo/registry/cache/x.crate`, a symlink to a real file outside it; after
  `sweep_sessions(&mut fx.host(false), ...)` the id is in `report.retired`, the cache directory
  is gone, the real file is intact.
- `a_late_retire_spares_the_successors_cache`: two Completed records of one stage and kind, the
  cache prepared for the first and then again for the second (the successor, still Running);
  sweeping retires the first and the cache survives with the marker naming the second.
- `a_running_session_keeps_its_session_cache` (the control: a Running record's cache survives).
- `a_failed_cache_release_keeps_the_capsule_for_the_next_tick`: make the cache root `0o555` so
  the removal fails (skip when running as root; restore `0o755` before the TempDir drops); the
  capsule is still on disk and the id is not in `report.retired`.

## 8. The test fixtures (no assertion line changes except where named)

- `tests_launch_capsule.rs` fixture (`:48-93`): create `repo/.git` as a directory; the
  `LaunchHost` literal gains `session: SessionFacts { git_common_dir: <repo/.git canonical>,
  cache_root: temp.path().join("session-caches"), real_caches:
  RealCaches::from_lookup(&temp.path().join("home"), &|_| None), credential_paths: Vec::new()
  }`. New test `a_stage_launch_grants_its_session_cache_and_exports_it` (exact name; acceptance
  runs it): a Stage launch's capsule holds `<cache_root>/stage-1-stage` in `allowWrite` and no
  `allowWrite` entry equal to `<cache_root>` itself (the root holds every other stage's cache),
  its marker `<cache_root>/stage-1-stage/.loom-session` and `repo/.git` in `denyWrite`,
  `repo/.worktrees` in `denyRead` and the worktree in `allowRead`; its wrapper holds
  `CARGO_HOME=<cache_root>/stage-1-stage/cargo` and `GIT_CONFIG_KEY_0=gc.auto`; the cache
  directory exists with mode `0o700` and its marker holds the session id. A second Stage launch
  gets the same path with the marker naming the new session. The control in the same test: a
  Knowledge launch (checkout) is granted `<cache_root>/stage-1-knowledge`, and has neither
  `repo/.git` in `denyWrite` nor any `.worktrees` entry in `denyRead`.
- `tests_confinement_e2e.rs` (the ownership exception above): the literal at `:85` gains
  `session: SessionFacts::resolve(&repo, base.join("session-caches"), &home, &|_| None).unwrap(),`
  and the import at `:13` becomes `use super::host::{LaunchHost, SessionFacts};`. That fixture
  runs `git init` before building the host, so `resolve` works there.
- `tests_session_settings.rs`: `checkout()` (`:178-198`) also creates `repo/.git`; `Checkout`
  gains the cache root (`root.join("session-caches")`); `write_capsule` (`:206-229`) fills
  `session: SessionPaths { git_common_dir: &<repo/.git>, cache_dir: &<cache root>/<session id>,
  codex_home: &<home>/.codex, credential_paths: &[] }` (the writer takes the cache path as given;
  no marker exists there, so none is listed).
- `tests_capsule.rs`: in `every_kind_gets_exactly_the_section_10_write_denies_for_its_location`
  (`:311-332`), the Stage and Adjudication kinds (both locations) also expect `{R}/.git` in
  `denyWrite` and `Edit(/{R}/.git/**)` in `permissions/deny` (add a constant beside
  `WORKTREE_DENIES` and include it for those kinds; the other kinds see none of the checkout
  metadata entries because the fixture's `.git` is empty). Replace
  `the_codex_lane_capsule_keeps_its_grants_and_denies_every_plugin_entry_beside_them`
  (`:334-370`) with `the_codex_lane_capsule_grants_its_session_codex_home_and_never_the_real_one`:
  with `<cache dir>/codex-home/config.toml` and `hooks.json` written by the test (the capsule
  writer does not seed), a codex Stage capsule's `allowWrite` holds the cache directory,
  `<home>/.codex/auth.json` and `~/.codex/plugin-data` but neither `~/.codex` nor
  `~/.claude/plugins/data/codex-openai-codex`; `denyWrite` holds `~/.claude/plugins`, both
  copies, and still `~/.codex/hooks`, `~/.codex/hooks.json`, `~/.codex/config.toml`; the control:
  a claude-only capsule holds `<home>/.codex/auth.json` in `denyRead` and not in `allowWrite`.
  These two are the plan's expected integrity events for this file.
- `tests_capsule_contents.rs`: `denies()` (`:45-56`) loses its `codex` parameter,
  `PLUGIN_ENTRIES` and the `plugin_entries` line; `try_build` (`:58-80`) passes
  `session_fs: &SessionFs::default()` and calls `denies(worktree_rooted)`. Add
  `the_session_layer_reaches_every_list` (a `SessionFs` with one distinct entry per list: each
  appears at its pointer, `edit_deny` under `permissions/deny`) paired with
  `a_default_session_layer_adds_no_allow_read_key`.
- `tests_capsule_interpreters.rs` (`:12-28`): `denies(false)` and
  `session_fs: &SessionFs::default()`.
- `tests_wrapper_env.rs`: `full_host_env` (`:53-59`) gains `session_env: Vec::new()` so the
  existing tests are unchanged. Add `every_kind_exports_the_session_cache_env_it_is_given`
  (exact name; acceptance runs it: `CARGO_HOME=/cache/s1/cargo` and
  `XDG_CACHE_HOME=/cache/s1/xdg-cache` in every kind's script) paired with
  `the_default_host_env_exports_no_cache_variable` (no `CARGO_HOME=`), and
  `session_env_values_are_shell_escaped` (a value with a space is single-quoted).

## Traps

- Every path in tests comes from a TempDir; no literal home, uid or repository path. The
  `/cache/s1/...` strings in `tests_wrapper_env.rs` are rendered text only, like the existing
  `/scratch/session1`.
- A cache root inside the repository's `.loom` would be write-denied by the capsule; fixtures
  put it beside the repository.
- `LaunchHost` has exactly one new field; do not add more (each one is another line in a file
  another stage owns).
- Remove the `plugin_entries` plumbing completely; `cargo clippy -D warnings` rejects an unused
  parameter.
- Doc comments describe the code as it is now.

## Check (once)

`cargo test --lib orchestrator::terminal::native::` from `loom/`. Report, never fix, a compile
error in a file you do not own, with its file:line and the owning worker (C1: `sandbox/session_cache/`;
C2: the rest of `sandbox/`, `fs/permissions/state_root.rs`, `codex.rs`).
