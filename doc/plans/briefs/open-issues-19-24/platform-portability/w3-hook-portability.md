# W3: portable hooks, BSD shims, CI (issue #24, shell side)

Read `doc/plans/briefs/open-issues-19-24/common.md` first. You are a sonnet worker of stage
`platform-portability`. Plan Decisions 6 and 10 are binding.

## Role and issue

On macOS, `wc -c <file` prints a left-padded count (`123`). Five hook sites test the raw
value against `^[0-9]+$`, so `subagent-stop.sh` exits before it appends a lifecycle row or runs the
review harvest, and `codex-forward-guard.sh` blocks every codex forward ("forwarder identity does
not match exactly one SubagentStart row"). Fix the helpers, put BSD tool behaviour under test on
Linux through a shim directory, and run the hook suite in CI in both modes.

## Files owned (the plan's W3 row; hook paths are repository-relative, Rust paths package-relative to `loom/`)

`loom-hooks/_lifecycle.sh`, `loom-hooks/codex-forward-result.sh`, `loom-hooks/codex-forward-guard.sh`,
`loom-hooks/subagent-stop.sh`, `loom-hooks/teammate-idle.sh`, `loom-hooks/tests/bsd-shims/wc`,
`loom-hooks/tests/bsd-shims/stat`, `loom-hooks/tests/bsd-shims/date` (new),
`loom-hooks/tests/_bsd_path.sh` (new), `loom-hooks/tests/run-all.sh`,
`loom-hooks/tests/subagent-stop-review-harvest.sh`, `loom-hooks/tests/subagent-stop-heartbeat-lock.sh`,
`tests/worker_evidence.rs`, `tests/worker_evidence/support.rs`, `tests/worker_evidence/setup.rs`,
`tests/codex_evidence/fixture_runtime.rs`, `tests/codex_evidence/happy_path.rs`,
`.github/workflows/ci.yml`. Write nothing else.

## Pinned interfaces (common.md and the plan, quoted)

- "Portable shell helpers live in `loom-hooks/_lifecycle.sh`. `loom_lifecycle_file_bytes` strips
  whitespace before the numeric test (never `$(( ))`). `loom_lifecycle_sha256` uses `sha256sum`,
  else `shasum -a 256`. Every padded-`wc` site and every bare `sha256sum` goes through them. The
  BSD epoch fallback accepts fractional seconds."
- "A skipped `loom-code-reviewer` stop writes one row to
  `.loom/work/subagents/<stage>/stop-skips.jsonl`." Row schema, pinned for W4 who reads it:
  `{"ts":"<UTC ISO>","agent_id":"<id>","agent_type":"loom-code-reviewer","reason":"<code>"}`.
- Contract (frozen, a TempDir `bin/` with a padded `wc` first on PATH):
  `loom_lifecycle_resolve_start <work> <stage> <parent> <loom_session> <agent> <expected_type>
  <observed> <hook>` must still resolve one exact start row in `<work>/subagents/<stage>/starts.jsonl`.
- The hook files are embedded in the binary by `include_str!` constants
  (`loom/src/fs/permissions/constants.rs`); a new top-level hook file would need registering there,
  which is not yours. All new shell goes inside `_lifecycle.sh` (369 lines now; stay at or under
  400) or `subagent-stop.sh`.

## Root cause sites (re-verified at HEAD)

`_lifecycle.sh`: `:163-164` (`loom_lifecycle_resolve_start`), `:205-206`
(`loom_lifecycle_transcript_evidence`), `:224` bare `sha256sum`, `:238-239`
(`loom_lifecycle_journal_ready`), `:105-108` (`loom_lifecycle_epoch`: BSD fallback parses only the
literal `.000Z`). `codex-forward-result.sh`: `:51-52` (`load_authorization`), `:86-87`
(`valid_persisted_output`), `:188-198` (`event_id`, duplicated sha256 branches). `subagent-stop.sh`:
`:55` dependency check, `:148` digest. `teammate-idle.sh`: `:16` dependency check, `:76` digest.
Not bugs, leave alone: `_lifecycle.sh:211-213`, every `tail -c 1 | wc -l` check, `post-tool-use.sh`,
`knowledge-orient.sh`, `_read_ledger.sh`, `_read_discipline.sh`, `commit-guard.sh`.
`codex-forward-guard.sh:246,327` fails only through `loom_lifecycle_resolve_start`, and
`forward_observed_at` through `loom_lifecycle_epoch`; fixing the helpers fixes the guard, so
`codex-forward-guard.sh` needs no edit unless you find one.

## Tasks

1. **Helpers in `_lifecycle.sh`** (bash 3.2 compatible: no `${x,,}`, `declare -A`, `mapfile`; keep
   regexes in variables):

   ```bash
   loom_lifecycle_file_bytes() {
    local bytes=""
    bytes=$(wc -c <"$1" 2>/dev/null) || return 1
    bytes=${bytes//[[:space:]]/}
    [[ "$bytes" =~ ^[0-9]+$ ]] && printf '%s\n' "$bytes"
   }
   ```

   `loom_lifecycle_sha256` reads stdin and runs `sha256sum` when present, else `shasum -a 256`
   (non-zero when neither exists); `loom_lifecycle_have_sha256` is the one-line availability test.
   Replace the four `wc -c` blocks with
   `bytes=$(loom_lifecycle_file_bytes "$f") || return 1` followed by the numeric bound
   (`((bytes <= 4194304)) || return 1`, `((bytes > 0)) || return 1`, `((bytes <= 8388608))`);
   `LIFECYCLE_TRANSCRIPT_BYTES=$bytes`. Replace `:224` with the helper. Budget: the file ends at or
   under 400 lines.
2. **Epoch fractions**: `loom_lifecycle_epoch` keeps the GNU `date -u -d` attempt; its BSD fallback
   splits the value with a regex into base `YYYY-MM-DDTHH:MM:SS`, optional `.digits` (dropped) and
   zone. `Z`: `date -j -u -f '%Y-%m-%dT%H:%M:%SZ' "${base}Z" +%s`. `+hh:mm`: remove the colon and use
   `date -j -f '%Y-%m-%dT%H:%M:%S%z' "${base}${zone}" +%s`. The equal-second fraction check in
   `loom_lifecycle_resolve_start` (`:188-190`) stays as is.
3. **Other sites**: `codex-forward-result.sh` `load_authorization` and `valid_persisted_output` use
   `loom_lifecycle_file_bytes`; `event_id` pipes into `loom_lifecycle_sha256` once (delete the
   duplicate branch). `subagent-stop.sh` and `teammate-idle.sh`: dependency check uses
   `loom_lifecycle_have_sha256`, digest uses `loom_lifecycle_sha256`. The acceptance command
   `rg -q -F 'sha256sum' loom-hooks/subagent-stop.sh loom-hooks/teammate-idle.sh` must find nothing,
   so reword the debug strings and comments there ("sha256 digest invalid", not the tool name).
4. **Skip ledger** in `subagent-stop.sh`: a function `loom_subagent_stop_skip <reason-code>` that
   returns unless `$AGENT_TYPE == $REVIEWER_AGENT_TYPE`, requires
   `loom_lifecycle_plain_path "$WORK_DIR/subagents/$LOOM_STAGE_ID" dir` (never creates a
   directory) and a non-symlink target, builds the row with `jq -nc` (`ts` from a fresh
   `date -u +%Y-%m-%dT%H:%M:%S.000Z`), and appends it with one
   `{ printf '%s\n' "$row" >>"$file"; } 2>/dev/null || true`. Diagnostic only: it never changes an
   exit code or output. Call it, after the existing `loom_debug`, at every `exit 0` skip that follows
   the work-dir and stage-binding checks (`:95-102`): codes `transcript_not_plain` (`:103`),
   `transcript_layout_mismatch` (`:113`), `timestamp_unavailable` (`:121`),
   `no_unambiguous_start_row` (`:130`, status 1), `lifecycle_defect` (`:130`, other status),
   `transcript_unusable` (`:138`, status 1), `event_digest_failed` (`:145-156`).
5. **BSD shims** in `loom-hooks/tests/bsd-shims/` (`#!/usr/bin/env bash`, bash 3.2, each finds the
   real tool by removing its own directory from `PATH`):
   - `wc`: run the real `wc "$@"`, then re-emit every leading integer field right-aligned in 8
     columns (`printf '%8d'`), keeping the rest of the line (BSD `wc -c <f` prints `5`).
   - `stat`: reject `-c` (`stat: illegal option -- c`, exit 1). Support `-f FORMAT FILE` by
     translating `%d`, `%i`, `%u` unchanged, `%z` to `%s`, `%m` to `%Y`, and calling the real `stat -c`;
     any other directive exits 1. No flag: delegate.
   - `date`: reject `-d` and `--date`. Support `-j -u -f FORMAT VALUE +OUTFMT` for formats built from
     `%Y %m %d %H %M %S %z` and literal characters: build an anchored regex from FORMAT (literals
     escaped, so `...%SZ` rejects `.123Z` exactly like BSD `strptime` with leftover text), parse VALUE,
     compute the epoch with the real `date -u -d "YYYY-MM-DD HH:MM:SS"` minus the `%z` offset, print per
     OUTFMT; a mismatch exits 1. Support `-r EPOCH +FMT` through the real `date -u -d @EPOCH`. Other
     invocations (`date -u +%Y-%m-%dT%H:%M:%S.000Z`) delegate.
   The repository copies need no executable bit (`mistakes/hooks-shell-portability.md`: "Repo hook
   scripts do not need the executable bit", and chmod is blocked under `loom-hooks/` in a stage).
   `tests/_bsd_path.sh` defines `bsd_shim_dir`: it copies the three shims with
   `install -m 0755` into a fresh `mktemp -d` and prints that directory; callers put it first on PATH
   and remove it.
6. **`run-all.sh`**: `source "$SCRIPT_DIR/_bsd_path.sh"`; when `LOOM_HOOK_TEST_BSD=1`, create the shim
   directory once, `trap 'rm -rf "$BSD_DIR"' EXIT`, print `BSD tool shims active`, and make `run_test`
   run `env -u ... PATH="$BSD_DIR:$PATH" bash "$script"` (build the extra argument in an array; guard
   with `if`, never a bare `cond && action` as a function's last statement, per the same knowledge
   file). Keep the existing `env -u LOOM_HOOK_PATH ...` list: `_read_discipline.sh` sets
   `PATH="${LOOM_HOOK_PATH:-$PATH}"`, which would swallow the shim directory.
7. **`subagent-stop-heartbeat-lock.sh`**: its line `[[ "$(wc -l <"$JOURNAL")" != "1" ]]` compares a
   raw `wc` string and fails under the padded shim. Do not edit that line (existing assertion lines
   are never edited): add, before it, a test-local function
   `wc() { command wc "$@" | tr -d '[:space:]'; }` with a comment that the test's own assertions must
   tolerate BSD padding (the hook runs in a separate `bash` process and never sees the function).
8. **Shell tests** appended before the final `echo "PASS"` in `subagent-stop-review-harvest.sh`
   (new lines only; source `tests/_bsd_path.sh` and `tests/_path_without.sh`):
   - a reviewer stop whose start row is missing writes exactly one `stop-skips.jsonl` row with
     `reason == "no_unambiguous_start_row"` and the agent id; a worker-type stop with no start row
     writes none.
   - in a subshell with `PATH="$(bsd_shim_dir):$PATH"` and `source _lifecycle.sh`:
     `loom_lifecycle_file_bytes` on a 5-byte file prints `5`; `loom_lifecycle_epoch
     2026-01-01T00:00:00.123Z` prints `1767225600`; `2026-01-01T02:00:00+02:00` prints `1767225600`;
     `loom_lifecycle_resolve_start` resolves a row built like `run_stop` builds it.
   - with `PATH="$(path_without sha256sum)"` (skip with an echo when `command -v shasum` fails),
     `printf abc | loom_lifecycle_sha256` starts with
     `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.
9. **Rust BSD variants** (include the shim files with `include_str!("../../../loom-hooks/tests/bsd-shims/wc")`
   and the same for `stat` and `date`; write them 0755 into a fixture-owned directory, never the repo):
   - `tests/worker_evidence/setup.rs`: `install_bsd_tools(dir)`; change the fixture `date` shim's
     fall-through line to `PATH="${FIXTURE_DATE_PATH:-/usr/bin:/bin}" exec date "$@"`.
   - `tests/worker_evidence/support.rs`: `Fixture::new_bsd(label)` (refactor `new` into a private
     `with_mode(label, bsd)`); a `bsd_bin: Option<PathBuf>` field; `configure` puts it after `bin` on
     PATH and sets `FIXTURE_DATE_PATH="<bsd_bin>:/usr/bin:/bin"` when present. Keep the file under 400.
   - `tests/worker_evidence.rs`, new tests: `bsd_padded_wc_records_lifecycle_and_settles` (mirror
     `exact_success_records_lifecycle_and_settles` on `new_bsd`) and
     `bsd_teammate_idle_after_a_stop_appends_to_a_non_empty_journal` (a stop, then an idle: two
     records, the second `claude_teammate_idle`).
   - `tests/codex_evidence/fixture_runtime.rs`: `Fixture::enable_bsd_tools(&self)` writes the shims
     into `self.root/bsd-bin`; `configure_child` prepends that directory to PATH when it exists. No new
     struct field (`fixture.rs` is not yours). `tests/codex_evidence/happy_path.rs`, new test
     `bsd_tools_record_the_same_terminal_identity`, `#[serial]`, mirroring
     `completed_job_preserves_request_and_terminal_identity` after `enable_bsd_tools`.
10. **`ci.yml`** job `hook-syntax`: after "Parse every shell hook" add a step installing
    `ripgrep fd-find jq` (`sudo apt-get update && sudo apt-get install -y ...`, then
    `sudo ln -sf "$(command -v fdfind)" /usr/local/bin/fd`), then steps `bash loom-hooks/tests/run-all.sh`
    and `LOOM_HOOK_TEST_BSD=1 bash loom-hooks/tests/run-all.sh`. Acceptance greps for the text
    `LOOM_HOOK_TEST_BSD=1` in the file.

## Patterns to copy

`tests/_path_without.sh` (helper style), `tests/subagent-stop-review-harvest.sh` `run_stop`
(row and payload construction), `tests/worker_evidence/setup.rs` `install_shims` and `write_exec`.
Do not copy the existing fixture `date` shim's `PATH=/usr/bin:/bin` fall-through into the BSD
variant: it would bypass the BSD shim.

## Traps (knowledge, quoted)

- `mistakes/hooks-shell-portability.md`: "`wc -c` output has leading spaces on BSD";
  "A Bash Function Ending in `cond && action` Aborts a `set -e` Script"; "Hook Tests Inherit the
  Live Session's Environment" (`LOOM_HOOK_PATH` splices the real PATH back in; tests unset it).
- BSD mode runs all 95 tests with the shims; a failure in a test you do not own is a real finding.
  Report it by name; do not edit that file or exempt it.
- Existing assertion lines in tests are never edited; add new lines and new tests.

## The one check

Run once, after your edits: `env LOOM_HOOK_TEST_BSD=1 bash loom-hooks/tests/run-all.sh 2>&1 | tail -40`
from the repository root (a shell suite; it needs no compiled crate). Report its tail.

## Report

Files changed; the check result; `_lifecycle.sh` final line count; failures in tests you do not own;
anything not done.
