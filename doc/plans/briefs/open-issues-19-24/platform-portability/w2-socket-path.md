# W2: socket path resolution and classification (issue #23)

Read `doc/plans/briefs/open-issues-19-24/common.md` first. You are a sonnet worker of stage
`platform-portability`. Plan Decision 5 is binding.

## Role and issue

On a worktree, every client joins `orchestrator.sock` onto the worktree's `.loom/work` symlink
spelling. That path is `R + stage id + 41` bytes; past 103 bytes (macOS) the connect fails with
`InvalidInput` before the syscall. `rpc::try_send_request` maps that to a hard error, and the
completion broker has its own copy of the connect that reports `daemon_transport`, so completion
ends `verified_pending_ack`. Fix: one resolver, used by every client, and a path that cannot fit
classifies as `DaemonReach::Unreachable`.

## Files owned (the plan's W2 row, package-relative to `loom/`)

`src/daemon/socket.rs` (new), `src/daemon/rpc.rs`, `src/daemon/rpc_tests.rs` (new),
`src/commands/stage/control_complete.rs`, `src/commands/stage/tests/control_complete.rs`,
`src/daemon/server/core.rs`, `src/daemon/server/shutdown.rs`,
`src/commands/status/ui/tui/daemon_client.rs`, `src/commands/status/web/broadcast.rs`,
`src/commands/repair/daemon_checks.rs`, `src/verify/review/observer.rs`,
`src/commands/init/execute.rs`, `src/commands/init/execute/tests.rs`,
`src/commands/status/ui/tui/app.rs`. Write nothing else.

## Pinned interfaces (common.md, quoted)

- "`pub const SOCKET_FILE: &str = "orchestrator.sock";`"
- "`pub const SUN_PATH_MAX: usize = 104;` (moved from
  `loom/src/daemon/server/lifecycle/socket_limit.rs`, which W1 deletes)"
- "`pub fn socket_path_fits(path: &Path) -> bool` (byte length strictly below `SUN_PATH_MAX`)"
- "`pub fn socket_path(work_dir: &Path) -> PathBuf` (canonicalized `work_dir` joined with
  `SOCKET_FILE`; on canonicalisation failure, the given spelling joined with it)"
- "`pub fn socket_path_problem(work_dir: &Path) -> Option<String>` (None when
  `socket_path(work_dir)` fits; else a message with the byte count, `104` and the path, and the
  advice to move the repository to a path of at most 74 bytes)"
- "The daemon's own bind keeps `work_dir.join(SOCKET_FILE)`; every client uses `socket_path`."
- W1 declares `mod socket;` and the `pub use` in `src/daemon/mod.rs` and calls
  `socket_path_problem` from `run_startup_preflights`. You never edit `daemon/mod.rs`.

## Root cause (re-verified at HEAD)

- `src/daemon/rpc.rs:64-66` private `socket_path` is `work_dir.join("orchestrator.sock")` on the
  caller's spelling; `:148-170` `try_send_request` maps `InvalidInput` through its catch-all
  `Err(e)` arm (`:163`).
- `src/commands/stage/control_complete.rs:72-81` has a private `send_request` that dials
  `work_dir.join("orchestrator.sock")` with `UnixStream::connect` and no classification.
- Other clients join the same spelling: `daemon/server/core.rs:116` (`check_status`),
  `daemon/server/shutdown.rs:24`, `status/ui/tui/daemon_client.rs:17` (via `tui/app.rs:118`),
  `status/web/broadcast.rs:229`, `repair/daemon_checks.rs:165`, `verify/review/observer.rs:70`
  (message text only).
- `completion_evidence.rs:196-204`, `state_relay.rs:37`, `contracts.rs:99`,
  `dispute_transport.rs:221`, `observer.rs:101` go through `rpc::try_send_request` and are fixed by
  the resolver inside it; do not edit them.

## Tasks (anchored by symbol)

1. **`daemon/socket.rs`** (about 60 lines plus tests): the constants and three functions above.
   `socket_path`: `work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf()).join(SOCKET_FILE)`
   (precedent `control_complete.rs:46-48`). `socket_path_problem` message, exactly this shape:
   `daemon socket path '<p>' is <N> bytes; AF_UNIX paths must be under 104 bytes. Move the
   repository to a path of at most 74 bytes.` Derive 74 from a const
   (`SUN_PATH_MAX - 1 - ".loom/work/orchestrator.sock".len() - 1`).
2. **`rpc.rs`**: delete the private `socket_path`; `use super::socket_path` and
   `super::socket_path_fits`. In `try_send_request`, after the `lstat` match and before the
   connect, return `Ok(DaemonReach::Unreachable)` when `!socket_path_fits(&socket_path)`. Order is
   binding: lstat `NotFound` gives `NotListening`; any other lstat error gives `Unreachable`; a
   path that does not fit gives `Unreachable`; then the connect mapping as today. Do not map every
   `InvalidInput`. Rewrite the doc comment "lives here, and only here" (`:132`) so it is true after
   your change (the daemon's own status/stop/TUI/web clients classify for themselves), add the
   too-long case to the `Unreachable` variant doc and to the `send_request` `Unreachable` message
   (`:181-186`).
3. **Move the tests**: cut the `mod tests` block of `rpc.rs` (`:200-367`) into
   `src/daemon/rpc_tests.rs` with every assertion line verbatim, and replace it in `rpc.rs` with
   `#[cfg(test)] #[path = "rpc_tests.rs"] mod tests;` (the path resolves beside `rpc.rs`, as
   `control_complete.rs` does with `tests/control_complete.rs`). The moved code uses
   `use super::*;`, which still sees `socket_path`, `try_send_request`, `ping` and so on.
4. **`control_complete.rs`**: delete `send_request`; `request_completion` calls
   `crate::daemon::send_request(work_dir, &request)` (note the argument order). Remove the now
   unused imports (`UnixStream`, `Duration`, `Context`, `read_message`, `write_message`); keep
   `Request`, `Response`, `read_user_token` (the tests file does `use super::*;`).
5. **Other clients**: `core.rs:116` and `shutdown.rs:24` use `crate::daemon::socket_path(work_dir)`;
   `core.rs:93` and `:194` use `SOCKET_FILE` (the bind path keeps `join`). `daemon_client::connect`
   keeps its `&Path` parameter but resolves first:
   `let socket_path = crate::daemon::socket_path(socket_path.parent().unwrap_or(Path::new(".")));`
   and reads the token from that resolved parent. `status/web/broadcast.rs:229` passes
   `&crate::daemon::socket_path(work_path)`. `tui/app.rs:118` replaces its
   `work_path.join("orchestrator.sock")` with `crate::daemon::socket_path(work_path)` (one line;
   the file is 391 lines, so change nothing else there). `daemon_checks.rs:165` uses
   `socket_path(work_dir)`. `observer.rs:70` prints `crate::daemon::socket_path(&self.work_dir)`.
6. **`init/execute.rs`**: after `let work_dir_path = work_dir.root();` (around `:122`), call a small
   helper `warn_on_long_socket_path(&repo_root, work_dir_path)` that prints
   `println!("  {} {problem}", "!".yellow().bold())` when
   `crate::daemon::socket_path_problem(&repo_root.join(work_dir_path))` is `Some`. Joining onto
   `repo_root` covers the relative root `WorkDir::new(".")` can return, and keeps `execute` from
   growing past one call line. The file is 350 lines; stay under 400. Add a test to
   `execute/tests.rs` (new lines only) that `warn_on_long_socket_path` reports a problem for a
   work root whose socket path is past the limit and nothing for a short one; make the helper
   return the message it printed (`Option<String>`) so the test needs no stdout capture.

## Tests to write (exact names)

- `src/daemon/socket.rs` inline `mod tests`: `socket_path_resolves_a_symlinked_state_root`
  (a TempDir symlink to a short real dir resolves to the real dir joined with `SOCKET_FILE`),
  `an_unresolvable_root_keeps_the_given_spelling`,
  `socket_problem_names_bytes_limit_and_path`,
  `socket_problem_is_none_for_a_short_root`, `socket_problem_measures_the_resolved_path` (a long
  symlink spelling over a short real root gives `None`).
- `src/daemon/rpc_tests.rs`, appended after the moved tests (all new lines):
  `a_socket_path_past_sun_path_is_unreachable` (a directory of one 90-character component under a
  TempDir holding a plain file `orchestrator.sock`: lstat succeeds, the path cannot fit, expect
  `Unreachable`; no bind needed), and `a_worktree_spelling_past_sun_path_is_answered`: bind
  `tmp/r/.loom/work/orchestrator.sock`, create `tmp/r/.worktrees/<80 x 'a'>/.loom/` and symlink
  `work` there to `../../../.loom/work`, assert the link spelling's socket path is at least 108
  bytes, serve one `Pong` from a thread exactly as `a_live_listener_is_answered` does, expect
  `Answered(Pong)`. Guard the bind with
  `crate::process::sandbox_probe::skip_unless(unix_socket_bindable(..), "<test path>", "<why>")`.
- `src/commands/stage/tests/control_complete.rs`, appended:
  `request_completion_reaches_the_daemon_through_a_long_worktree_spelling` (same layout as above,
  pattern of `reads_the_user_token_through_a_symlinked_work_dir`; the listener thread reads one
  `Request` and replies `Response::Ok`; assert `request_completion(...)` is `Ok`); sandbox-guarded.
- `src/commands/repair/daemon_checks.rs` `mod tests`, appended:
  `a_daemon_child_command_line_is_a_loom_run_command_line`: `is_loom_run_cmdline("/usr/local/bin/loom
  run --daemon-child /repo/.loom/work")` is true, and an unrelated `loom runner` line is false.

## Patterns to copy

`rpc.rs` tests `:284-366` (real listener, shut down before drop, sandbox guard);
`control_complete.rs` tests `:43-63` (symlinked work dir). Do not copy `core.rs` `check_status`'s
`UnixStream::connect(&socket_path)` shape into new code: classification belongs in
`try_send_request`.

## Traps

- Acceptance greps: `rg -q -F 'UnixStream::connect' src/commands/stage` must find nothing. Do not
  write that text in code, tests or comments anywhere under `src/commands/stage` (tests may bind a
  `UnixListener`; they never dial with `UnixStream::connect`, use `try_send_request` or
  `request_completion`).
- Knowledge, `mistakes/live-state-pollution.md` and `mistakes/detached-spawn-in-tests.md` (cited in
  common.md): tests use TempDirs only, never the live `.loom/work`, never a surviving process.
  A Linux stage sandbox denies `AF_UNIX` socket creation: every bind or dial test must be guarded by
  `skip_unless`, or it passes for the wrong reason.
- `daemon/server/tests.rs:18` expects the bind path unresolved; never canonicalize the daemon's own
  `socket_path` field.
- Existing assertion lines are never edited. Moving them verbatim is allowed.
- `rpc.rs` is 367 lines now; after the move it is about 200 and must stay under 400.
- Completion is never spooled (no `StageRequest` completion variant exists; spooling would be
  forgeable). Do not add one.

## The one check

None before the wave returns (the crate does not compile until W1 and W5 finish).

## Report

Files changed; any client you could not move to `socket_path` and why; the `tui/app.rs:118` note;
deviations from the pins.
