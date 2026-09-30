# Brief: Close the Paths a Sandboxed Stage Agent Can Still Write or Read Outside Its Worktree

You own this from a cold start. Your deliverable is a loom plan, written through
`/loom-plan-writer`, at `doc/plans/PLAN-sandbox-escape-hardening.md`. Do not implement anything:
report the plan's location and stop (CLAUDE.md Rule 1). `loom run` executes it later.

## The question that started this

The operator asked whether the worktree-isolation hook is bypassable: "the agents just use bash to
write a file outside the worktree." The answer was checked from inside a live stage session
(integration-verify of PLAN-source-graph-mechanism, 2026-09-30). The short version:

- The hook, `loom-hooks/worktree-file-guard.sh` (PreToolUse on Read/Write/Edit/MultiEdit/NotebookEdit/Glob/Grep), does not
  see Bash. That part of the suspicion is right.
- Bash is confined by a different layer: Claude Code's OS sandbox (bubblewrap on Linux), configured
  from loom's per-session capsule `W/capsules/<session-id>.settings.json`
  (`orchestrator/terminal/native/session_settings.rs`, built by `sandbox::settings::build_settings`).
  Everything outside the write allow-list is mounted read-only, and loom refuses
  `sandbox.enabled=false` (`loom/src/sandbox/config.rs:156-166`). So Bash cannot write to arbitrary
  paths: the main checkout, other worktrees, `$HOME`, `~/.claude`, `.loom/work`, `.git/hooks` and
  `.git/config` all refused writes.
- The allow-list still contains host-shared locations outside the worktree, and reads are barely
  confined. Those are the holes below.

## Verified findings

Probe method: `test -w` / `test -r` from Bash inside the stage session. `access(2)` returns `EROFS`
on a read-only bind, so the probe writes nothing. Reproduce all of it yourself before designing
(script at the end).

| Path (`R` = the main checkout; `~` = the operator's home) | Result in a stage sandbox |
| --- | --- |
| `R`, `R/loom/src`, `R/.worktrees`, `R/.loom/work`, `R/.loom/cache` | read-only |
| `$HOME`, `~/.bashrc`, `~/.claude`, `~/.cargo/bin`, `~/.local/bin`, `~/.config` | read-only |
| `R/.git/hooks`, `R/.git/config` | read-only (loom's `denyWrite`) |
| `R/.git/worktrees/<stage>/commondir`, `R/.git/worktrees/<stage>/config.worktree` | read-only, but not from loom: the capsule has no deny for them. The mechanism is unidentified (Claude Code's own worktree handling is the likeliest candidate); find and confirm it rather than relying on it |
| `R/.git/worktrees/<stage>/gitdir`, `HEAD`, `index` | **writable** |
| `R/.git`, `R/.git/HEAD`, `R/.git/refs/heads`, `R/.git/refs/heads/main`, `R/.git/packed-refs` | **writable** |
| `R/.git/objects`, `R/.git/info`, `R/.git/worktrees` | **writable** |
| `~/.cargo/registry` (incl. `registry/src`), `~/.bun/install/cache`, `~/.npm` | **writable** |
| `/tmp/claude-<uid>` | writable (harness scratch; out of scope, see decisions) |
| `~/.config/gh/hosts.yml` (GitHub token), `~/.netrc`, `~/.npmrc` | **readable** |

### H1. The shared git directory is writable at the top level

The grant does not come from loom. The capsule's `sandbox.filesystem.allowWrite` has no `.git`
entry; Claude Code adds the git common dir to the allow-list itself for a linked-worktree session.
Loom narrows it only with two `denyWrite` entries, `.git/hooks` and `.git/config`
(`loom/src/sandbox/control_surfaces/session_denies.rs:81-82`). A deny wins over an allow, so more
`denyWrite` entries are the lever.

What a stage agent can do today:

- rewrite `R/.git/HEAD`, `refs/heads/main` or `packed-refs`, moving `main` (or what the main
  checkout has checked out) to any commit it made, with no merge and no review;
- replace a loose object under `R/.git/objects/` with different content under the same name, which
  git does not re-hash on every read, so later host checkouts can see altered history content;
- rewrite its own `R/.git/worktrees/<stage>/gitdir`, and must never gain `config.worktree`: host-side
  git that loom runs in a worktree reads it when `extensions.worktreeConfig` is on
  (`loom/src/git/worktree/pinned.rs:18-20`), so a writable one would put `core.fsmonitor` or a
  filter driver on the host; it is unset in this repository today, and the plan must not depend on
  that;
- write `R/.git/info/*` and other stages' `R/.git/worktrees/<other>/` metadata, and other stages'
  branch refs under `refs/heads/loom/`.

What a linked-worktree commit legitimately needs (measure it, do not assume): objects, its own
branch ref `refs/heads/loom/<stage-id>` plus its `.lock` in `refs/heads/loom/`, the reflog under
`logs/refs/heads/loom/`, and its own `worktrees/<stage>/` (HEAD, index, logs, ORIG_HEAD,
COMMIT_EDITMSG).

### H2. Package caches are writable, and the host builds from them

`loom/src/sandbox/package_caches.rs` grants the per-user caches of cargo, rustup, bun, npm, pnpm,
yarn, deno, uv, pip and go so `cargo add` / `bun install` work inside a stage. The capsule shows the
full list (`~/.cargo/registry`, `~/.cargo/git`, `~/.cargo/.package-cache*`, `~/.rustup/downloads`,
`~/.rustup/tmp`, `~/go/pkg`, `~/.cache/go-build`, ...). An agent can edit an extracted crate under
`~/.cargo/registry/src/*/<crate>-<ver>/` (for example its `build.rs`); cargo does not re-verify
extracted sources, so the next host `cargo build` of any project using that crate runs the code on
the host, outside every sandbox. The same class applies to any cache a host tool links or copies
from without re-verifying. `~/.cargo/registry` was 1.8 GB on the machine where this was found, so
copy cost matters.

### H3. Credential reads are confined to five paths

The OS `denyRead` list is `CREDENTIAL_DENY_READ_PATHS` (`loom/src/fs/permissions/state_root.rs:51`:
`~/.ssh`, `~/.aws`, `~/.config/gcloud`, `~/.gnupg`, `~/.claude/.credentials.json`) plus loom's state
tokens (`models/stage/types.rs::default_deny_read`, `sandbox/settings/policy.rs::deny_read_patterns`).
Readable today: `~/.config/gh/hosts.yml`, `~/.netrc`, `~/.npmrc`. Check the rest yourself
(`~/.git-credentials`, `~/.docker/config.json`, `~/.kube/`, `~/.pypirc`, `~/.cargo/credentials*`,
`~/.config/hub`, `~/.config/glab-cli`, `~/.terraform.d/credentials.tfrc.json`, `~/.azure/`,
`~/.local/share/keyrings/`, password-manager CLIs). Network egress goes through a filtering proxy
with a per-plan domain allow-list, which narrows exfiltration but does not remove it (a registry
publish to an allowed domain is one channel).

### H4. Other worktrees and the main checkout are readable from Bash

Rule 10 forbids a stage from reading outside its worktree, but only the file tools enforce it. This
is an isolation gap between stages, not a boundary against the operator. Decide whether the plan
closes it (a `denyRead` on `R/.worktrees/<other>` and on the main checkout's source, keeping what a
stage must read: its own worktree, `R/.git`, the `.loom/work` symlink target it is allowed) or
records it as an accepted gap.

## Decisions the operator already made

1. **Package caches: per-session cache copy.** Each stage session gets its own writable cache
   (per-session `CARGO_HOME`, `BUN_INSTALL_CACHE_DIR`, `npm_config_cache`, pnpm store, yarn cache,
   `UV_CACHE_DIR`, `PIP_CACHE_DIR`, `GOMODCACHE`/`GOCACHE`, `DENO_DIR`, rustup download dirs) layered
   over a read-only view of the operator's caches, so installs still work and the real caches are
   never written by a sandboxed session. The host never builds from a session cache.
2. **`/tmp/claude-<uid>` stays writable.** The Claude Code harness uses it for session scratchpads,
   task output and tool-result spill files. Out of scope.
3. **Nothing is machine- or user-specific.** Loom runs for other operators: the uid in
   `/tmp/claude-<uid>`, the home directory, the repository root and every cache location differ per
   machine. Derive each at runtime (the process uid, `dirs::home_dir`, `git rev-parse
   --git-common-dir`, the tool's own env overrides such as `CARGO_HOME`), never from a literal, and
   make tests build their paths the same way.
4. **This is its own plan**, not bundled into another plan's stages.

## What the plan must settle

1. **The exact `.git` write set.** Measure which paths git writes for every git operation a stage
   session runs (`git add`, `git commit`, `git status`, what `loom stage complete` runs in the
   worktree) with `strace -f -e trace=openat,rename,mkdir,unlink,link` or `inotifywait -r` on a
   temp repo with a linked worktree. Then choose:
   - deny-list (add `denyWrite` for `HEAD`, `packed-refs`, `info/`, `refs/` except the stage's own,
     `worktrees/<other>`, `logs/HEAD`) on top of the harness grant; check that Claude Code's
     `denyWrite` accepts globs or needs enumerated paths, and what it does with paths that do not
     exist yet (lock files);
   - how to stop one stage writing another stage's `refs/heads/loom/<other>` when git needs to
     create `<ref>.lock` in the shared `refs/heads/loom/` directory (a per-stage ref namespace such
     as `loom/<stage-id>/head` is one option; weigh its blast radius on merge code);
   - object integrity: whether sandbox-written objects can be trusted at merge. Options include a
     per-stage object store (the stage writes to its own `objects` with the shared store as a
     read-only alternate; the host fetches the stage branch at merge) or verifying the stage
     branch's reachable objects (`git fsck` / `rev-list --objects` plus re-hash) before merging.
     Pick one with measured cost.
2. **Per-session caches** (decision 1): the mechanism per tool (env relocation is the lever; the
   signal text already notes that a cache relocated by env var is not covered by the current
   grants), how the read-only view is provided (reflink copy, copy of the compressed `.crate` /
   index only, or tool-native read-only sources), cold-start cost per stage, where the copies live,
   cleanup at stage end, and the rustup toolchain boundary. Remove the corresponding grants from
   `package_caches.rs`.
3. **Read denies** (H3): the list to add to `CREDENTIAL_DENY_READ_PATHS`, and the platform limit
   (`sandbox/settings/policy.rs:171` `host_supports_deny_read`). Read
   `concerns/sandbox-and-confinement-gaps.md:115`, heading
   ``## No `Read(...)` Deny Rule May Exist in Any Settings File (2026-09-04)``
   first: the OS `denyRead` layer and Claude Code `Read(...)` permission rules are different things,
   and the latter is forbidden for a documented reason.
4. **H4**: close or accept, with the reason.
5. **The codex lane**: check whether `loom-hooks/codex-forward.sh` sessions run under the same OS
   confinement or codex's own sandbox policy, and bring them to the same bar.
6. **Regression tests that run where the threat lives.** Unit tests on `build_settings` output are
   necessary and not sufficient. Add an end-to-end check that runs inside a real sandboxed stage
   session and asserts `EROFS` for each forbidden write and a refused read for each credential
   path; `concerns/sandbox-and-confinement-gaps.md#Sandbox Denial Has No End-to-End CI Canary`
   records that none exists today. Acceptance criteria must be written from inside a sandboxed
   worktree (`mistakes/verification-harness.md#Write Acceptance Criteria From Inside a Sandboxed
   Worktree, Not From Your Checkout`).

## Where to read first

- `CLAUDE.md` and `doc/loom/knowledge/INDEX.md`; then only the sections they point to.
- `doc/loom/knowledge/architecture/security-and-isolation.md`: `## Worktree Isolation (4-Layer
  Defense)`, `## Security Model`, `## Where a Session's Write Grants Come From`.
- `doc/loom/knowledge/concerns/sandbox-and-confinement-gaps.md` (all sections; several are this
  topic), `concerns/agent-rule-bending-hardening.md`, `mistakes/sandbox-and-settings.md`,
  `mistakes/sandbox-state-channels.md`.
- Code: `loom/src/sandbox/` (`settings.rs`, `settings/policy.rs`, `control_surfaces.rs`,
  `control_surfaces/session_denies.rs`, `package_caches.rs`, `grant_paths.rs`, `config.rs`),
  `loom/src/fs/permissions/state_root.rs`, `loom/src/models/stage/types.rs::default_deny_read`,
  `loom/src/orchestrator/terminal/native/session_settings.rs`, `loom-hooks/worktree-file-guard.sh`
  and its tests under `loom-hooks/tests/`.
- A live capsule to compare against: `.loom/work/capsules/<session-id>.settings.json` of any running
  stage (`sandbox.filesystem.allowWrite`, `denyWrite`, `denyRead`).

## Probe script

Run it from Bash inside a sandboxed stage session (a scratch plan is fine). It writes nothing.

```bash
# The main checkout: the parent of the shared git dir, whatever machine or user runs this.
R="$(cd "$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd)"
G="$(git rev-parse --path-format=absolute --git-dir)"   # this worktree's metadata dir
for p in "$G/gitdir" "$G/commondir" "$G/config.worktree" "$G/HEAD" \
  "$R" "$R/.worktrees" "$R/.loom/work" "$R/.git" "$R/.git/HEAD" "$R/.git/refs/heads" \
  "$R/.git/refs/heads/main" "$R/.git/packed-refs" "$R/.git/objects" "$R/.git/info" \
  "$R/.git/worktrees" "$R/.git/hooks" "$R/.git/config" "$HOME" "$HOME/.claude" \
  "$HOME/.cargo/bin" "$HOME/.cargo/registry/src" "$HOME/.bun/install/cache" "$HOME/.npm" \
  "$HOME/go/pkg" "$HOME/.cache/uv"; do
  if [ -e "$p" ]; then [ -w "$p" ] && echo "WRITABLE  $p" || echo "read-only $p"; else echo "absent    $p"; fi
done
for f in "$HOME/.config/gh/hosts.yml" "$HOME/.netrc" "$HOME/.npmrc" "$HOME/.git-credentials" \
  "$HOME/.docker/config.json" "$HOME/.kube/config" "$HOME/.pypirc" "$HOME/.cargo/credentials.toml"; do
  [ -r "$f" ] && echo "READABLE  $f" || echo "denied/absent $f"
done
```

After the plan lands, the same script is the acceptance check: every line for H1, H2 and H3 paths
must read `read-only` or `denied/absent`, while a `git commit` in the stage worktree, a
`cargo add` of an uncached crate and a `bun install` still succeed.
