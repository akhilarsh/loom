# P3: the plan-writer skill: environment inventory, `provision`, and stage exits

Stage `plan-environment`, wave 1, tier sonnet. Read `../common.md` first. This is prose in
`skills/loom-plan-writer/` (from `loom/`, the paths start `../skills/`). No Rust.

You own `skills/loom-plan-writer/SKILL.md` and its references `sandbox.md`, `v2-contracts.md`,
`authoring-detail.md` and `grounding-protocols.md`.

## Why

The owner's intent: a stage agent disputes when a check is wrong and blocks only for needs
that truly require a person, and plan creation checks network and other human-dependent
requirements up front, so a person is rarely needed. The skill today tells authors to "run
`loom repair`, merge with suggestions" (wrong: `loom repair` prints no domain suggestions;
`loom init` does, `commands/init/plan_setup.rs:136-175`), never mentions `loom stage block`,
frames a dispute as a cost, and tells a stage agent to "stop and report".

## Settled facts to write from (do not invent others)

- New plan field, `version: 2` only: `loom.provision: [{ working_dir: "<dir>", command:
  "<cmd>" }]`. Before every stage session spawns (first spawn, retry, handoff successor,
  requeue after a verdict), the daemon runs each command on the host, in
  `<worktree>/<working_dir>`, in order, 600 s each, with the stage-command environment (`HOME`
  and `PATH` kept, credentials dropped). A failure blocks the stage, and `loom status` shows
  `provision`<cmd>` in `<dir>`failed: <stderr tail>`. Commands must be idempotent (every
  spawn runs them again); `bun install --frozen-lockfile`, `npm ci`, `uv sync --frozen` and
  `pnpm install --frozen-lockfile` are. The daemon reads the entries from the plan file at each
  spawn, so an operator can add one and run `loom stage retry`. Provision runs outside the
  sandbox: its registry needs no `allowed_domains` entry.
- New `loom plan verify` errors (v2): a command in a stage's acceptance, setup, wiring tests,
  after-stage checks or dead-code check that fetches from a registry its sandbox does not
  allow (`bunx`/`npx`/`npm|bun|pnpm|yarn install` → `registry.npmjs.org`; `cargo
  install|fetch` → `crates.io`, `index.crates.io`, `static.crates.io`; `uv sync|add`, `uv pip
  install`, `uvx`, `pip install` → `pypi.org`, `files.pythonhosted.org`; `go get`, `go mod
  download` → `proxy.golang.org`); the repository's pre-commit hook (from `core.hooksPath`,
  else `.git/hooks`) fetching from a registry a stage does not allow; a JS package with a test
  runner that no `provision` entry covers. `loom init` prints domain suggestions.
- `loom stage dispute-criteria <stage-id> [--field acceptance|wiring|wiring-tests]
  --criterion-index <n> --reason "..."`; `loom stage complete` labels failures `[criterion
  n]`, `[wiring n]`, `[wiring_tests n]`.
- `loom stage block <stage-id> "<what is needed and why>"`: from a stage agent, the daemon
  retires the session, the exit is not a crash, `loom status` shows the reason, and the operator
  resumes with `loom stage retry <stage-id>` after providing what was needed.
- Filing any dispute ends the stage session by design; the daemon starts a fresh session with
  the verdict. Impact-selected tests have no dispute: a failure there is a regression.
- BLOCK-F, which stage `stage-exits` puts in every stage signal (quote it only if you need it,
  byte for byte):

  > **When the stage cannot finish.** Fix what the stage can fix. A criterion, wiring check,
  > contract, review finding or test-integrity event that is wrong gets a dispute: `loom stage
  > dispute-criteria` (with `--field` for wiring and wiring-tests entries), `dispute-contract`,
  > `dispute-findings` or `dispute-integrity`. Commit your work first. Filing ends this session
  > by design: the daemon starts a fresh session with the verdict, so waiting gains nothing.
  > Never revert, weaken or postpone correct work to avoid a dispute. A need only a person can
  > meet (a credential, a host install, a network domain or path the plan does not grant) gets
  > `loom stage block <stage-id> "<what is needed and why>"`: the daemon retires the session and
  > shows the reason to the operator. Never end a turn asking the operator to act while the
  > stage is executing.

## Edits

1. **SKILL.md Section 8** (`## 8. Sandbox & Execution Environment`, about line 411):
   - Replace "Then run `loom repair`, merge with suggestions, and add a `sandbox` block" with:
     `loom init` prints domain suggestions; `loom plan verify` now errors on the registry,
     hook and JS-provision gaps it can see.
   - Add an **environment inventory** checklist, walked per stage across the stage's whole
     life, where each need ends as allowed (a sandbox domain or path), provisioned (a
     `loom.provision` entry), or resolved with the user before `loom run`: implementation
     fetches (`cargo add`, `bun add`, `uv add`, `go get`), acceptance commands,
     impact-selected runners per package (a JS package needs `node_modules`: provision it),
     the repository's git hooks (a pre-commit `bunx` needs the npm registry in every stage
     that commits), audit databases (`cargo audit` fetches from github.com unless run with
     `--no-fetch` against a host copy), credentials and tokens, and host tools that must be
     installed.
   - Document `loom.provision` with a YAML example, the idempotence rule, when it runs, and
     that it runs outside the sandbox.
2. **SKILL.md Section 9**: keep the dispute text, add `--field` for wiring and wiring-tests
   entries, say a dispute ending the session is expected and the daemon starts a fresh one
   with the verdict, and name `loom stage block <stage-id> "<what is needed and why>"` for a
   need only a person can meet, with what the daemon does. Wording agrees with BLOCK-F.
3. **SKILL.md Pre-STOP checklist** (about line 673): add a line "Environment inventory done:
   every network, install, credential and host need of every stage allowed, provisioned or
   resolved with the user".
4. **references/sandbox.md**: a `provision` section (what, when, host-side, idempotent, how it
   differs from `setup:`, which only prefixes acceptance commands), and the pre-commit hook's
   network need.
5. **references/v2-contracts.md** line about 128 (Disputes bullet) and about 138 (Dispute in
   batches): say filing ends the session by design and a fresh session starts with the verdict;
   keep the batching advice (one `dispute-findings` per round). Name `--field` in the
   zero-test-guard row's `dispute-criteria` mention if you touch it. Do NOT edit the
   Impact-selected tests row or bullet: knowledge-distill rewrites those after this plan
   merges.
6. **references/authoring-detail.md** Section 9 (about line 116): replace "A stage agent
   facing one is correct to stop and report rather than weaken the check — its sanctioned move
   is ..." with the dispute route (BLOCK-F's wording), and name `loom stage block` for a need
   only a person can meet.
7. **references/grounding-protocols.md** "JS/TS projects: provision worktree dependencies
   first": the plan's `loom.provision` entry is how a worktree gets `node_modules` before a
   session starts; an in-session `bun install` remains the fallback when the daemon cannot
   provision (and needs `registry.npmjs.org` plus a `node_modules` write grant).

## Constraints

- `orchestrator/signals/tests_doctrine.rs` pins BLOCK-B verbatim in SKILL.md Section 4 and
  sweeps SKILL.md for retired phrases; `tests_doctrine_v2.rs` requires SKILL.md to name `loom
  project detect` and `references/v2-contracts.md`. Leave Section 4 and those names alone.
  Never write "report a needed sandbox block as a blocker", "STOP and report it as a blocker",
  or "ends your turn and sends the stage to adjudication; loom retires" (stage `stage-exits`
  retires them).
- Every fenced code block names its language. Tables stay aligned. Writing style: plain
  sentences, no marketing words, no "not X, but Y" constructions.
- Acceptance greps SKILL.md for `loom.provision` and `loom stage block`, and
  `references/sandbox.md` for `provision`.

## Check

None: P1 edits the crate in parallel. The main agent runs
`cargo test --lib orchestrator::signals::tests_doctrine`, which reads SKILL.md.
