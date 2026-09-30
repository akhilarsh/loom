# Plan: Source graph mechanism — evidence, dialects, views and evaluation

## Overview

This plan implements `doc/PROPOSAL-source-graph-mechanism.md`. It has four parts:

- **Evidence.** The source graph becomes an evidence-bearing index. Every reference
  edge carries its call sites and an evidence class. Ambiguity survives as candidate
  sets instead of a last-spelling-wins guess. Duplicate declarations get distinct ids.
  Coverage is reported by dialect, files and bytes.
- **Languages.** A dialect registry replaces the `DetectedLanguage` coupling. Loom
  gains TSX, JavaScript/JSX, Java, C#, Ruby, PHP, C and C++ through default-on grammar
  packs.
- **Speed and delivery.**
  - Cross-file resolution follows imports, aliases, package scope and receivers, and
    refuses name-only binds it cannot justify.
  - `loom map` gets honest direct/transitive semantics, call-site rows, freshness
    states, a portfolio census, source windows and timings, all read from a persisted
    resolved view whose incremental relink is proven equal to a cold build.
  - Retrieval routes by query intent, explains its graph neighbours and attaches
    anchored windows.
- **Measurement.** A labelled edge-quality evaluator with published thresholds, plus
  an operator protocol for the cross-project agent comparison.

The shared design lives in `doc/plans/briefs/source-graph-mechanism/design.md`, and
every worker brief points into it. Where prose and YAML differ, the YAML is
authoritative for acceptance, files and contracts.

## Goals and non-goals

Goals:

- Output never claims stronger evidence than its producer has. Only containment reaches
  confidence 1.0.
- A caller row shows `caller path:call line → target path:declaration line`, the
  written callee, the evidence class and the site count.
- A missing, stale or never-built graph is labelled as such everywhere and never
  triggers a detached rebuild loop.
- Warm `loom map` queries stop re-resolving the whole graph in every process.
- New languages ship behind explicit capability and gap reporting, and are gated by
  labelled corpora.

Non-goals (settled with the user):

- Kotlin and Swift are dropped: their grammars live outside `github.com/tree-sitter`.
- Wave D (shell and configuration languages).
- A new storage engine: canonical JSON stays, and `--timings` measures it.
- Persisted name and adjacency indexes (proposal item 4). The view persists
  resolution, stats and the dependency index only. The query paths
  (`direct_neighbors`, `reverse_adjacency`, `ResolvedGraph::node`) make one linear
  pass over a graph the process has already parsed. A persisted derived index would
  grow the JSON that every warm query parses and would need a query-consumer switch
  in files no stage owns. The storage-engine decision, taken on `--timings` data,
  decides both.
- Running the agent-task comparison. It needs consenting projects and live agents; this
  plan ships its protocol and tools.
- Compatibility periods or migration maps: the project is unreleased, so old caches are
  discarded through the graph schema version and CLI defaults change in place.

## Decisions

`design.md` sections 1–15 settle every decision. The ones later stages build on:

- **D-evidence.** `EdgeProvenance` becomes seven classes, strongest first:
  `structural`, `compiler` (reserved), `receiver`, `import`, `local-name`,
  `unique-name`, `syntax`. Confidence is a documented evidence ranking, not a
  probability.
- **D-sites.** Edges merge repeated calls into one edge with every site span.
- **D-ids.** Colliding ids gain `@<sig8>`, and `symbol_key` holds the base id.
- **D-dialects.** A 12-row `DIALECTS` table drives extractor lookup. `.h` belongs to
  C++. Packs: `source-graph` (core plus TSX and JS), `source-graph-wave-b` (Java, C#,
  Ruby, PHP), `source-graph-wave-c` (C, C++), all default-on. A missing pack reports a
  named gap.
- **D-resolution.** Seven ordered rules. Cross-family binding is never allowed, and
  dynamic receivers are never bound by name. An external import or glob refuses
  unique-name binding, and ambiguity is kept as a candidate set of at most 8. Traversal
  follows candidates at 0.2, so impact recall and `reachable` checks do not regress.
- **D-states.** `current | stale | never built | unavailable` on every surface. A
  failed build serves the newest older base labelled stale.
- **D-view.** A persisted resolved view per snapshot, keyed by schema, revision,
  generation, extractor digest and resolver version. `relink` is byte-equal to
  `build_cold`, and relinks only from a previous view of the same identity (a
  changed extractor digest or resolver version builds cold). No persisted
  `GraphIndex`.
- **D-local.** Same-file binding (design 6.2) checks scope eligibility before
  counting matches, and a spelling that a named import binds is left to the
  resolver.
- **D-import-spec.** `ImportBinding.path` is a lossless module spec: C/C++ keep a
  leading `<` for a system include, and Ruby `require_relative 'x'` is written
  `./x`, so `require 'x'` and `require_relative 'x'` reach the resolver as different
  specs.

## Baseline evidence

Measured on the host at `67e442d5`, in the main checkout, not a stage sandbox:

| Command | Result |
| --- | --- |
| `cargo test --no-fail-fast` (full suite, `loom/`) | 6403 passed, 0 failed, 13 ignored |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `cargo check --no-default-features --lib` | clean |
| `cargo test --lib context::` | 522 passed |
| `cargo test --lib map::` | 18 passed |
| `cargo test --lib commands::knowledge::eval` | 31 passed |
| `cargo test --test maintainability` | 8 passed |

Filters used in stage acceptance, counted at HEAD with `cargo test --lib -- --list`:

| Filter | Tests |
| --- | --- |
| `context::` | 522 |
| `verify::goal_backward::` | 51 |
| `verify::impact_tests` | 2 |
| `commands::hook::` | 174 |
| `commands::knowledge::` | 160 |
| `orchestrator::signals::` | 216 |
| `plan::schema::` | 249 |

Filters naming modules this plan creates (`context::extract::treesitter::`,
`context::view::`, `context::census::`, `context::eval_edges::`) select zero tests at
HEAD by design: they go green only when the stage writes them.

Other measured facts the gates rely on:

- The whole `--all-targets` suite runs in about 57 s warm, well inside the 300 s cap
  on a simple acceptance entry.
- `cargo test --lib -- --list` at HEAD counts 27 tests under `context::extract::`, 6
  under `context::coverage::` and 55 under `commands::run::`.
- The maintainability baseline is an exact-match ledger in both directions
  (`tests/maintainability/baseline.rs`): shrinking a baselined item fails the test as
  surely as growing one.

Grammar crates were checked on crates.io while planning:

- all seven new crates depend on `tree-sitter-language ^0.1`, which is compatible
  with the pinned `tree-sitter 0.27.0`;
- each ships generated `parser.c` and a `queries/tags.scm`;
- `tree-sitter-typescript 0.23.2`, already pinned, exports `LANGUAGE_TSX`;
- `tree-sitter-javascript` is not in the local cargo cache, so stage `language-packs`
  needs the crates.io domains its sandbox allows.

The new grammars add about 88 MB of generated C source:

| Grammar | `parser.c` size |
| --- | --- |
| C# | 29.7 MB |
| C++ | 17.3 MB |
| Ruby | 15.3 MB |
| PHP | 7.2 MB + 6.8 MB |
| C | 3.9 MB |
| JavaScript | 2.9 MB |
| Java | 2.6 MB |

The existing four grammars total 29 MB. Stage `language-packs` records the measured
release-binary size and build time, before and after.

## Cross-plan constraints

- `PLAN-web-host-graft-followthrough.md` is partially executed: its web host (worker
  W) and usage fixture (L1, L2) are on main. Its memory-prepare (M), retrieval (R) and
  CLI (C) workers are pending, and R and C overlap this plan. Its YAML owns
  `loom/src/cli/**` (so `cli/types_ops.rs` as well as `dispatch.rs`),
  `loom/src/context/rank.rs`, `loom/src/context/rank/**`, `loom/src/context/tests/**`,
  `loom/eval/retrieval-cases.yaml`, `orchestrator/signals/cache.rs`,
  `orchestrator/signals/cache/**`, `tests_doctrine.rs` and
  `loom/maintainability-baseline.txt`. This plan writes into that territory in three
  stages:
  - `graph-contract`: W1's renames in `context/tests/{rank_source_expand,
    rank_source_expand_fusion, source_fixtures, retrieve_source, pack_required}.rs`;
  - `map-api-freshness`: the `HookCommands::ReconcileGraph` arm in `cli/dispatch*.rs`,
    the `--cancel` field in `cli/types_ops.rs`, and `context/tests/{store,
    retrieve_source}.rs`;
  - `retrieval-delivery`: `rank.rs`, the new `context/rank/candidate.rs`,
    `context/tests/**` and `retrieval-cases.yaml`.
  Pending worker C splits `dispatch.rs` into `dispatch_commands.rs`, so the arm moves.
  Locate it with `rg -n 'HookCommands::ReconcileGraph' loom/src/cli`, never by line.
- `PLAN-model-router-hooks.md` (unexecuted) owns `loom/src/cli/dispatch.rs` and
  `loom/src/cli/types.rs`. Its integration-verify owns `loom/src/**`, and its
  knowledge-distill owns `README.md`, `CONTRIBUTING.md` and `doc/loom/knowledge/**`,
  the same files as this plan's knowledge-distill.
- **Do not run this plan concurrently with either sibling.** Run it after they merge
  or before they start.
  - **If a sibling merged first:** each stage that owns an overlapping file re-reads it
    on the merged tree before its workers start, and briefs workers with the current
    symbol anchors.
  - **If this plan merged first:** web-host-graft's R and C briefs are stale. R treats
    `rank_source/candidacy.rs` and `user_prompt_compose.rs::clears_item_floor` as
    fixed, but this plan changes both (D1, D3). This plan also adds `StageQuery.surface`,
    `RankedCandidate.via`, `ContextPack.text_search`, `PackRequest.text_search` and
    `SelectionReason::SymbolQuestion`, and moves `RankedCandidate` and `RankQuery` into
    `context/rank/candidate.rs`. C's dispatch split must keep
    `ReconcileGraph { cancel }`. Before that plan runs, refresh its R and C briefs.
  - Whichever plan runs second runs both plans' retrieval regression families in its
    retrieval stage: `context::tests::rank_source`,
    `context::tests::retrieval_natural_language` (once it exists) and
    `commands::hook::`.
- `PLAN-secure-distilled-loom-v2.md` and `PLAN-adopt-ecc-best-practices.md` are prose
  only. They have no executable stages and no committed surface this plan depends on.
- `PLAN-loom-state-confinement.md`, `PLAN-strengthen-verification.md` and
  `PLAN-codex-model-fallback.md` are already on main, and this plan works within their
  rules:
  - the stage sandbox denies writes to `<repo>/.loom/cache`, which the view memory
    fallback covers (design 12);
  - `~/.cargo/registry` is granted (`sandbox/package_caches.rs`);
  - the unwired-file check skips `/tests/` paths, so fixtures and corpora don't trip it.

## Before `loom run`

The plan, `doc/plans/briefs/source-graph-mechanism/**`,
`doc/PROPOSAL-source-graph-mechanism.md` and the knowledge corrections this plan relies
on (`doc/loom/knowledge/{INDEX.md, architecture/source-graph.md,
entry-points/context-and-source-graph.md,
concerns/automatic-knowledge-source-graph-followups.md, mistakes*.md, mistakes/*.md}`)
must be committed on main first. A stage worktree is cut from `HEAD`: an untracked
brief does not exist there, and every worker would start blind
(`mistakes/verification-harness.md`, "Untracked plan and worker briefs leave a
worktree stage blind"). The operator commits these; `loom run` does not.

The operator also runs `cargo audit` once in `loom/` on the host. That fetches the
RustSec advisory database into `~/.cargo/advisory-db`. The sandbox allows no
github.com traffic, so stage `language-packs` and integration-verify run
`cargo audit --no-fetch` against that copy. The sandbox grants write access only to
the database's lock file, `~/.cargo/advisory-db..lock`. If `cargo audit --no-fetch`
still fails with a permission or fetch error, the stage records a blocker memory
naming the denied path. It never drops the check.

## Gate conventions (every code stage)

These rules come from the pressure test and bind every stage below.

- **`working_dir: "loom"`.** Loom runs each contract as `cargo test <name> -- --exact`,
  with no `--manifest-path`, from the stage's `working_dir`
  (`testrun/adapters/cargo_test.rs::single_test_command`, `verify/contracts/site.rs`).
  The repository root has no `Cargo.toml`, so `working_dir: "."` makes every
  contract exit 101. At freeze that exit reads as red, and at completion it fails the
  stage. The seven standard stages therefore run in `loom/`.
  - Every YAML path in `files`, `acceptance`, `before_stage`, `after_stage`,
    `artifacts`, `wiring`, `wiring_tests`, contract `file` and the worker tables' "Files
    owned" column is package-relative (`src/...`, `tests/...`, `Cargo.toml`).
  - Paths outside the package are written `../skills/...` and `../doc/...`. They
    appear only in `files:`, worker tables and shell commands, because `artifacts`,
    `wiring.source` and contract `file` reject `..`.
  - Prose paths in descriptions, AMENDMENTS, briefs and `design.md` stay
    repo-relative (`loom/src/...`). Brief paths are repo-relative, from the worktree
    root.
  - integration-verify and knowledge-distill keep `working_dir: "."`.
- **Warm-up.** Each stage worktree has its own cold `loom/target`, and sccache is
  withheld under the sandbox. A cold `cargo build --all-targets` takes about 154 s, a
  cold `cargo test --all-targets` about 215 s, and two cold stages running side by side
  roughly double both. Each stage's main agent therefore starts
  `cargo build --all-targets` in the background (Bash `run_in_background`) before
  briefing wave 1, and runs every acceptance command once in-session before
  `loom stage complete`. `cargo build --all-targets` stays the first acceptance entry,
  because the 30 s `after_stage` and `wiring_tests` cargo commands reuse its artifacts.
  Feature sets keep separate artifacts in `target/`, so the degraded builds do not
  invalidate the default build.
- **Timeouts.** A simple-string `acceptance` entry gets 300 s. An extended acceptance
  entry, and every `before_stage`, `after_stage` and `wiring_tests` command, gets 30 s
  (`verify/criteria/runner.rs`, `verify/goal_backward/truths.rs`,
  `verify/goal_backward/wiring_tests.rs`). Every acceptance entry therefore stays a
  simple string. Cargo commands in `after_stage` and `wiring_tests` rely on the
  acceptance run having built the test and bin targets first; acceptance runs before
  them.
- **The canonical gate, every stage.** Every code stage runs the gate of
  `loom/.githooks/pre-push`, adapted to `working_dir: "loom"`:
  - `cargo test --all-targets --no-fail-fast` (about 57 s warm);
  - `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
  - clippy and fmt.
  Named `--exact` tests and module filters stay as diagnostics beside the full suite.
  They never replace it. A scoped subset let a stage break a test it neither owns nor
  runs (`commands/run/tests/preflight.rs` under graph-contract,
  `context/tests/retrieve_source.rs` under map-api-freshness). Two gate items apply
  only where they can observe something:
  - `cargo audit --no-fetch` runs in `language-packs`, the one stage that changes
    `Cargo.lock`, and in integration-verify (see "Before `loom run`").
  - `scripts/flake-check.sh` runs with the filter of a stage's own timing-sensitive
    tests: `commands::hook::reconcile_graph::` in map-api-freshness. Its default
    filters (`quota::`, `process::`, `verdict_apply_tests::`,
    `stalled_judge_tests::`) run in integration-verify, because no code stage edits
    those modules. Each filter is its own acceptance entry, so each gets its own 300 s
    cap.
- **Degraded builds are linted.** Every `--no-default-features` build in acceptance
  is a `cargo clippy ... --lib --bins -- -D warnings`, never a bare `cargo check`,
  because `cargo check` exits 0 with warnings. `--bins` covers the binary, which
  `CONTRIBUTING.md` requires for extraction and grammar changes.
- **Contracts are proven red by mutation.** A Rust contract file is one binary. Each
  contract file here names symbols that do not exist at its freeze, so every contract
  freezes as `build_failed`, which proves nothing about its assertions. Before the final
  review round, the stage's main agent takes each contract in turn, applies the
  implementation its `rejects:` names (or reverts the key line), confirms the contract
  fails, restores the tree, and records
  `loom memory note "mutation: <contract-id> red under <mutation>"`.
- **Worker amendments.** Each stage description ends with an `AMENDMENTS` block. It
  overrides the worker briefs and `design.md` where they differ. The main agent pastes
  each worker's amendment lines into that worker's spawn prompt, after the brief path,
  and names the amendment lines of other workers the worker's files depend on.
- **Anchors.** Every edit is anchored by symbol. Where a brief gives a line number,
  locate the symbol with `loom map --outline` or `rg` and ignore the number.

## Execution diagram

```mermaid
graph LR
    graph-contract --> language-packs & binding-resolver
    binding-resolver --> map-api-freshness
    map-api-freshness --> resolved-view
    binding-resolver --> resolved-view
    resolved-view --> retrieval-delivery
    map-api-freshness --> retrieval-delivery
    language-packs & binding-resolver & resolved-view --> edge-quality-eval
    retrieval-delivery & edge-quality-eval --> integration-verify
    integration-verify --> knowledge-distill
```

`language-packs` runs in parallel with `binding-resolver`, `map-api-freshness` and
`resolved-view`. `retrieval-delivery` and `edge-quality-eval` run in parallel.

## Knowledge bootstrap: skipped

`doc/loom/knowledge/` is populated for this subsystem:

- `architecture/source-graph.md`
- `architecture/context-retrieval*.md`
- `entry-points/context-and-source-graph.md`
- `concerns/automatic-knowledge-source-graph-followups.md`

The stale claims found while planning were corrected in place before this plan was
written. Every stage's Knowledge Brief quotes those topics.

## Stage necessity

- **graph-contract.**
  - Q1: every later stage builds on its evidence types, dialect table and schema
    version, and three of them start in parallel from its merged tree.
  - Q4: with any other stage it passes the 500k context budget.
- **language-packs.**
  - Q4: eight new extractors, seven dependencies and pack features are one stage's
    worth of review on their own.
  - It shares no file with `binding-resolver`, `map-api-freshness` or
    `resolved-view`, so it runs beside them.
- **binding-resolver.** Q2: it rewrites `resolve.rs`, which `map-api-freshness` also
  edits (declarations and the `pub use` line). Q4: merged with `language-packs` the
  combined review exceeds the budget.
- **map-api-freshness.** Q2: after `binding-resolver`, for `resolve.rs`. Q1:
  `resolved-view` needs its snapshot states and `commands/map.rs` surface merged.
- **resolved-view.** Q1: retrieval and evaluation read the view. Q2: it edits
  `commands/map.rs`, `refresh/snapshot.rs` and `graph_store/fallback.rs` after
  `map-api-freshness`.
- **retrieval-delivery.** Q4: routing, expansion, rendering, windows and eval cases
  are a large review on their own. Q2: it edits `retrieve/graph.rs` after
  `map-api-freshness` and `resolved-view`.
- **edge-quality-eval.** Q1: it needs every extractor and the final resolver merged
  (`language-packs`, `binding-resolver`). Q2: it edits `commands/map.rs` after
  `resolved-view`.

## Stages

### 1. graph-contract: evidence model and dialect registry

Waves (worker table in YAML):

1. **Wave 1: W1 (sonnet).** Types and signatures from design 2–5 exactly, then a
   mechanical crate-wide migration so everything compiles and passes under the new
   vocabulary.
2. **Wave 2: three workers.**
   - W2 (opus): harness semantics (sites, local binding rules, candidate sets,
     receiver binding, the new capture protocol including `@definition.qualifier`,
     disambiguated ids);
   - W4 (sonnet): schema checks, corrupt-layer recovery, worktree staleness;
   - W5 (sonnet): coverage by dialect, bytes and gaps.
3. **Wave 3: W3 (sonnet).** Rust, TypeScript, Python and Go queries: receivers,
   import statements with bindings, `extractor_version` bumps, test tables.

W1 also applies renames in files later-wave workers own. Waves are sequential, so no
two workers hold a file at once.

Risk walk:

| Area | Contract |
| --- | --- |
| Untrusted input (source bytes; a multi-byte identifier in a hashed signature) | `duplicate-declarations-get-distinct-ids` |
| External data (layers persisted by an older loom) | `corrupt-base-layer-is-rebuilt` |
| Lifecycle (rebuild after a corrupt layer, a second run) | `corrupt-base-layer-is-rebuilt` |
| The behaviour the stage exists for | `ambiguous-local-call-keeps-candidates`, `repeated-calls-keep-every-site`, `only-containment-is-certain`, `out-of-scope-or-imported-name-is-not-bound-locally` |

Settled by the pressure test (details in the stage's `AMENDMENTS`):

- Local binding checks scope eligibility before it counts matches. A single nested
  `hidden::helper` never binds a call from outside `hidden` (D-local).
- A spelling whose first segment is the local name of a named import is left
  unresolved, with no candidates, for the resolver's import rule.
- The worktree stale-parser regression has a fixed name,
  `stale_parser_base_entry_is_reextracted`, which stage `resolved-view` re-runs after
  it switches the worktree graph to the view.

Filesystem, process scale, configuration and user-facing reachability do not apply:
the entry is `extract_file` and `ensure_snapshot`, both driven directly.

Expected test-integrity events: `TI-edit` on the existing source-graph test files whose
provenance and confidence assertions this stage rewrites (design 3.1 renames):

- `extract/rust/tests.rs`, `extract/tests.rs`, `source_graph/tests.rs`;
- the inline test asserts on `EdgeProvenance::Inferred` in `extract/typescript.rs`,
  `extract/python.rs` and `extract/go.rs`;
- `resolve/tests_*.rs`, `map/views/tests.rs`, the `map/views/json.rs` key-set tests,
  `graph_store/tests.rs`;
- `refresh/tests_source_graph/*`, `tests/integration/source_graph_fixtures.rs`.

`commands/run/tests/preflight.rs` changes a `GraphLayer` literal (W4 sets
`schema_version`). That is not an assertion line, so it raises no event.

The stage files **one** `dispute-integrity` listing every such event, after its final
review round, with the reason quoting design 3.1.

### 2. language-packs: TSX, JavaScript and grammar packs

- **Wave 1: L0 (sonnet).** Seven exact-pinned `cargo add` calls, the `[features]`
  block (the one hand edit; no cargo command manages it), pack flags, and eight
  registered declaration-level extractors.
- **Wave 2: four workers.**
  - L1 (sonnet): TSX and JavaScript (JSX references, CommonJS `require`, a shared TS
    query macro);
  - L2 (sonnet): Java and C#;
  - L3 (sonnet): Ruby and PHP;
  - L4 (sonnet): C and C++.
- The main agent measures `cargo build --release` size and time before wave 1 and after
  wave 2, and records both with `loom memory note`.

Risk walk:

| Area | Contract or check |
| --- | --- |
| External data (grammar node names from real crates) | every contract extracts real source through the registered grammar |
| Configuration propagation (a pack feature off gives a named gap) | acceptance runs `cargo test --no-default-features --features source-graph --lib context::extract::`, where L0's gated unit tests assert the gap detail; a contract cannot run a non-default feature set |
| Reachability through the registry | `wave-pack-dialects-extract-declarations` |
| The behaviour the stage exists for | `tsx-file-yields-symbol-nodes`, `jsx-element-becomes-reference-edge`, `commonjs-require-is-an-import-binding`, `java-overloads-get-distinct-ids` |

Import specs (D-import-spec): L3 writes Ruby `require_relative 'x'` as path `./x`
(a spec already starting `./` or `../` is kept as written) and `require`/`load` specs
verbatim. L4 keeps a leading `<` for a system include. binding-resolver's R1 reads
the same encoding. The two stages run in parallel, so the encoding is fixed in both
stages' AMENDMENTS. The end-to-end proof (both `app/x.rb` and `lib/x.rb` present) is
in edge-quality-eval's Ruby corpus.

Expected integrity events: none. Existing tests are not edited, apart from the
registry test's stage-1 `.tsx` gap assertion. Its removal is a `TI-edit` event, disputed
with the reason "TSX is now registered (design 5.3)".

### 3. binding-resolver: language-aware resolution

- **Wave 1: R1 (sonnet).** Per-dialect module path conventions, namespace index,
  package-scope files.
- **Wave 2: R2 (opus).** The ordered rules and refusals of design 7, family-keyed
  indexes, candidate sets, and the recording API of design 7.0 that `resolved-view`
  depends on.

Risk walk:

| Area | Contract |
| --- | --- |
| External data correctness (each language's import semantics) | `import-alias-binds-the-aliased-definition`, `self-receiver-binds-across-files`, `external-glob-import-refuses-unique-name` |
| Untrusted input (dynamic receivers, cross-language names) | `dynamic-receiver-call-is-not-bound-by-name`, `names-never-bind-across-families` |

Expected integrity events: `TI-edit` in `resolve/tests_resolve.rs` and
`tests_qualified.rs` where a rule changed an outcome. R2 lists each one, and the stage
files one dispute. One is certain: the `./language` import test expects an ambiguity
that design 7.1's relative resolution no longer produces.

Settled by the pressure test (details in the stage's `AMENDMENTS`):

- Extraction-time candidate sets are final. Resolution never touches them, so relink
  cannot lose them.
- Key recording and `touched_keys` share one function, so relink selects every edge
  a cold build would re-resolve. Namespace lookups record `ns:` keys.
- A Rust path is external unless its first segment names a local module, so
  `use serde::de::*` refuses unique-name binding.
- Ruby: a `./`- or `../`-prefixed spec (from `require_relative`) resolves against the
  importing file's directory only. A bare spec (from `require` or `load`) resolves to
  `lib/<spec>.rb` when that file exists, else to any `<spec>.rb` suffix match. Test
  `ruby_require_and_require_relative_pick_different_files`.

### 4. map-api-freshness: map API, freshness states, census, windows

- **Wave 1: three workers.**
  - M1 (sonnet): direct neighbours with sites and candidates, and candidate-aware
    impact with an evidence filter;
  - M2 (sonnet): `GraphState`, stale serving, the reconcile lease with backoff,
    pending and `--cancel`;
  - M3 (sonnet): `context::census` and `context::window`.
- **Wave 2: M4 (sonnet).** The `loom map` command surface of design 10.

Risk walk:

| Area | Contract |
| --- | --- |
| Reachability from the entry point (the binary `loom map`) | `callers-json-reports-call-site-lines`, `map-json-carries-schema-and-snapshot`, `impact-labels-substring-fallback`, `map-timings-json-reports-phases-and-peak-rss` |
| Lifecycle (never-built must not trigger a rebuild) | `never-built-graph-does-not-request-rebuild` |
| Filesystem paths and untrusted input (a user-supplied `--window` id) | `window-rejects-ids-outside-the-graph` |
| Process I/O and scale (`git check-attr` over thousands of paths) | `census-handles-thousands-of-paths` |
| External data (gitattributes, generated markers) | `census-reports-exclusions-beside-eligible` |

Expected integrity events: `TI-edit` in these files, disputed once:

- `commands/tests_map.rs` (the `require_view` message);
- the `map/views/json.rs` key-set tests;
- `tests_neighbors.rs` (callers are calls-only now);
- `brief_tests.rs` and `tests_freshness.rs` (freshness words);
- `context/tests/retrieve_source.rs` (a never-built, empty graph is degraded now).

Settled by the pressure test (details in the stage's `AMENDMENTS`):

- M2 owns every `Freshness` and `SnapshotOutcome` literal its fields break, including
  `context/tests/store.rs` and `commands/knowledge/bootstrap/tests.rs`.
- `wants_rebuild` has one truth table, pinned by the contract.
- The lease backoff counts failures across spawns (read-modify-write). A run's
  failure comes from its real `SnapshotOutcome`. The failure test drives a real
  snapshot failure and checks the next eligibility time.
- The holder keeps its pid through the pending pass. A finish or pending clear writes
  only while the lock still names the holder. The hook's exit stays best-effort (0);
  failure accounting is internal.
- Bootstrap's `require_snapshot` still refuses a failed build, including one that
  serves a stale base: a failed build keeps `action: Unavailable`.
- The census streams `git check-attr --stdin -z` through a writer thread, and its
  contract uses paths long enough to exceed both ARG_MAX and the pipe buffer.
- Binary tests spawn through `helpers::loom_cmd()`, which the spawn guard requires.

### 5. resolved-view: persisted resolved view

- **Wave 1: V1 (opus).** Identity, dependency index, `build_cold`, `relink`, and
  equivalence tests (7 scripted changes, a 50-step seeded sequence, and identity
  changes over unchanged bytes).
- **Wave 2: V2 (sonnet).** Persistence with memory fallback, per-snapshot
  materialization, pruning with a 2 GiB budget, the `bytes_read` counter, and the reader
  switch. After it, only `context/view` calls `resolve_graph`.

Risk walk:

| Area | Contract |
| --- | --- |
| Lifecycle (incremental equals cold; a corrupt view is rebuilt, not served) | `relink-equals-cold-build-after-namesake-added`, `relink-equals-cold-build-after-target-removed`, `corrupt-view-file-is-rebuilt-not-served` |

Expected integrity events: `TI-edit` in the `map/views/json.rs` key-set tests, where
V2 adds the snapshot's `resolver_version` and `view` keys and `timings.view`. The
stage files one dispute for them. V2 owns those keys under `src/map/views/**`.

Lifecycle decisions settled by the pressure test (details in the stage's `AMENDMENTS`):

- **Identity.** A view relinks only from a previous view with the same schema,
  extractor digest and resolver version; otherwise it is built cold. Copying a
  previous entry because its `content_hash` matches is sound only under an equal
  identity. `identity_change_relinks_cold` pins this for three cases, each with
  identical content hashes: different parser output, a newly enabled pack, and a
  resolver-version bump.
- **No persisted indexes.** The view holds the graph, stats and dependency index
  (non-goals).
- **Warm path.** A second process reads the materialized view and runs no
  resolution. `warm_view_load_runs_no_resolution` counts resolver runs, and
  `warm_second_run_reads_the_materialized_view` checks the binary. Removing the
  literal `resolve_graph` call is not enough on its own.
- **Disk round trip.** `view_round_trips_through_disk_with_provenance_counts`
  reloads a view through a fresh `GraphStore` with non-empty `by_provenance`.
- **Missing base.** A view is never persisted for a revision with no base layer, and
  publishing a base deletes stale views for that revision.
- **Read-only readers.** The worktree graph loads views through a loader that never
  writes.
- **Parse budget.** One `loom map` process parses the view at most once.
- **Cleanup.** Pruning and `loom clean` remove views together with their base.

### 6. retrieval-delivery: intent routing, explained neighbours, windows

- **Wave 1: D0 (sonnet).** Splits `schema.rs`, `pack.rs` and `retrieve.rs`, all at the
  size cap. Adds every new type and field, and creates the six test files wave 2 fills,
  each with one real test.
- **Wave 2: three workers.**
  - D1 (opus): `classify`, routing, `TextSearchHint::for_pattern`;
  - D2 (sonnet): explained, token-capped expansion and caveats;
  - D3 (sonnet): rendering, windows by surface, hook floor, view-backed retrieval,
    eval cases.

Risk walk:

| Area | Contract |
| --- | --- |
| Untrusted input (query text turned into a shell command) | `literal-hint-escapes-single-quotes` |
| The behaviour the stage exists for | `symbol-question-admits-a-one-word-name`, `literal-query-is-classified-with-its-text`, `graph-neighbors-carry-their-edge`, `relationship-query-seeds-direct-callers` |
| Negative guard (prose must not admit a symbol) | `prose-with-a-node-name-is-not-a-symbol-question` |

The negative guard is a contract. `retrieval_delivery_contracts.rs` names symbols that
do not exist at freeze, so the whole file freezes as `build_failed`, and a negative
contract is legal. Freezing it stops D1 from weakening the guard in-stage. D1's 12
negative phrasings and the unchanged candidacy tests stay acceptance tests as well.

Expected integrity events: `TI-edit` in `brief_tests.rs` (source-section assertions),
disputed once if raised.

### 7. edge-quality-eval: labelled corpora and thresholds

- **Wave 1: E3 (sonnet).** The evaluator, `loom map --eval-edges` (pure, in memory),
  thresholds, and the operator protocol `doc/source-graph-evaluation.md`.
- **Wave 2: two workers.** E1 (sonnet) and E2 (sonnet) write 12 labelled corpora from
  language semantics, then run the evaluator.
- **Wave 3, only if E1/E2 report loom defects.** One `loom-senior-software-engineer`
  (opus) fixes each defect at its source in `extract/**` or `resolve/**`. The main
  agent writes its brief from the reports; relabelling to match loom is forbidden.

Risk walk:

| Area | Contract |
| --- | --- |
| External data (hand-written label files) | the evaluator's `deny_unknown_fields`, which E3's unit tests cover |
| The behaviour the stage exists for | `false-high-confidence-is-counted`, `impact-false-negatives-are-counted-by-depth`, `every-dialect-meets-published-thresholds` |
| A thin or empty corpus, a loosened thresholds file | `thin-corpus-fails-thresholds`; `every-dialect-meets-published-thresholds` pins the threshold values |

Expected integrity events: `TI-edit` in `commands/tests_map.rs` (the `require_view`
message gains `--eval-edges`), disputed once.

The Ruby corpus carries the end-to-end check of D-import-spec. `app/x.rb` and
`lib/x.rb` both define `helper`. `app/a.rb` does `require_relative 'x'` and
`app/b.rb` does `require 'x'`, and each then calls `helper()`. The labels expect
`app/x.rb#function:helper` from `a.rb` and `lib/x.rb#function:helper` from `b.rb`.
A resolver that conflates the two statements gives a false high-confidence edge,
and `every-dialect-meets-published-thresholds` fails.

The thresholds file and the corpora are not test files, so the test-integrity gate
cannot see an edit to them. The frozen contracts pin the threshold values. Relabelling
is forbidden by the description and checked in review.

### Integration verification

- The full canonical gate: suite, clippy, fmt, rustdoc, `cargo audit --no-fetch`,
  `scripts/flake-check.sh` with its default filters, and the degraded builds.
- The merged tree re-runs every stage's wiring.
- Parallel `loom-code-reviewer` subagents cover security (the window path handling, the
  reconcile `--cancel` signal, census git calls), architecture (evidence-class
  honesty, view identity), and test coverage. Fixes go through sonnet or opus engineers
  by the rubric, and every finding is fixed or disputed.
- Functional smoke:
  - `loom map --eval-edges` over every corpus;
  - `loom map --help` lists every new view;
  - the binary contracts and `loom/tests/map_cli.rs` drive `loom map` against temp
    repos.
- Operator-side checks that need the shared cache stay outside acceptance (listed
  below).

### Knowledge distillation

Curates every memory into knowledge:

- `architecture/source-graph.md`: rewritten to the new evidence classes, dialects,
  packs, view and states;
- `architecture/context-retrieval*.md`, `entry-points/context-and-source-graph.md`,
  `stack.md` (grammar packs and pins), and `concerns` (known gaps: macros, templates,
  reflection, `#private` members, Ruby bare calls, the storage-engine decision awaiting
  `--timings` data, persisted name and adjacency indexes deferred to that decision);
- the README `loom map` section.

It records unimplemented reviewer suggestions.

### Post-merge operator checks (not acceptance: they need the shared cache or live agents)

1. `loom map --census` in this repository, and `--root <other checkouts>` across the
   portfolio.
2. `loom map --find-all run_views --timings` twice (cold, then warm), with the numbers
   recorded for the storage decision.
3. `loom knowledge eval`, for the new retrieval cases.
4. `scripts/retrieval-ab` against the pre-plan binary.
5. The agent-task comparison per `doc/source-graph-evaluation.md`.

---

<!-- loom METADATA -->

```yaml
loom:
  version: 2
  ratchet_files:
    - loom/maintainability-baseline.txt
  sandbox:
    enabled: true
    auto_allow: true
    filesystem:
      deny_read: ["~/.ssh/**", "~/.aws/**", "~/.config/gcloud/**", "~/.gnupg/**"]
      allow_write:
        - "loom/src/**"
        - "loom/tests/**"
        - "loom/eval/**"
        - "loom/target/**"
        - "loom/Cargo.toml"
        - "loom/Cargo.lock"
        - "doc/**"
        - "skills/**"
        - "~/.cargo/advisory-db..lock"
    network:
      allowed_domains: ["crates.io", "index.crates.io", "static.crates.io"]
      allow_local_binding: false
      allow_unix_sockets: []
  stages:
    - id: graph-contract
      name: "Evidence model and dialect registry"
      summary: "Edges gain call sites, evidence classes and candidate sets; duplicate declarations get distinct ids; a dialect table replaces the language coupling; stale or corrupt graph caches rebuild instead of wedging."
      stage_type: standard
      skills: ["loom-rust"]
      subagent_timeout_secs: 900
      working_dir: "loom"
      dependencies: []
      description: |
        Implement design sections 2-6 and 8 of doc/plans/briefs/source-graph-mechanism/design.md.
        Use parallel subagents and skills to maximize performance.
        Spawn every worker BY AGENT TYPE with the fixed prompt plus "Your brief: <path>. Read it in full before anything else."
        Territories are DISJOINT within a wave. Workers NEVER spawn subagents.
        Waves are sequential: W1; then W2, W4, W5 in ONE message; then W3.
        W1 also applies mechanical renames in files that wave-2/3 workers own later (listed in its brief).

        | Worker | Role | Tier | Files owned | Shared context | Brief path |
        | ------ | ---- | ---- | ----------- | -------------- | ---------- |
        | W1 | Types and migration | sonnet | src/context/source_graph/mod.rs; src/context/source_graph/edge.rs; src/context/source_graph/node.rs; src/context/source_graph/imports.rs; src/context/source_graph/tests.rs; src/context/extract/mod.rs; src/context/extract/dialect.rs; src/context/extract/lexical.rs; src/context/extract/tests.rs; src/context/graph_store/layer_types.rs; src/verify/goal_backward/definition_sites.rs; src/context/resolve.rs | design.md 2-5 | doc/plans/briefs/source-graph-mechanism/graph-contract/w1-foundation.md |
        | W2 | Harness semantics | opus | src/context/extract/treesitter/mod.rs; src/context/extract/treesitter/build.rs; src/context/extract/treesitter/collect.rs; src/context/extract/treesitter/ids.rs; src/context/extract/treesitter/binding.rs; src/context/extract/treesitter/tests.rs | design.md 3, 4, 6 | doc/plans/briefs/source-graph-mechanism/graph-contract/w2-harness.md |
        | W3 | Existing language queries | sonnet | src/context/extract/rust.rs; src/context/extract/rust/tests.rs; src/context/extract/typescript.rs; src/context/extract/python.rs; src/context/extract/go.rs; tests/integration/source_graph_fixtures.rs | design.md 3.4, 5.3, 6 | doc/plans/briefs/source-graph-mechanism/graph-contract/w3-languages.md |
        | W4 | Schema and corrupt layers | sonnet | src/context/graph_store/mod.rs; src/context/graph_store/tests_corrupt.rs; src/context/refresh/snapshot.rs; src/context/refresh/source_graph.rs; src/context/refresh/source_graph/layer.rs; src/context/refresh/tests_schema.rs; src/context/worktree_graph.rs; src/context/worktree_graph_tests.rs | design.md 2 | doc/plans/briefs/source-graph-mechanism/graph-contract/w4-store-lifecycle.md |
        | W5 | Coverage dimensions | sonnet | src/context/coverage/mod.rs; src/context/coverage/dialects.rs; src/context/coverage/tests.rs; src/map/views/json.rs | design.md 5, 8 | doc/plans/briefs/source-graph-mechanism/graph-contract/w5-coverage.md |

        CONTRACT SURFACE (the contract session writes loom/tests/graph_contract_contracts.rs from this, before any code):
        - loom::context::extract::{registry, extract_file}: extract_file(&registry(), Path::new("src/lib.rs"), bytes) -> FileExtraction { nodes, edges, coverage, imports }. extract/mod.rs declares `pub mod dialect;`, so loom::context::extract::dialect::{DIALECTS, GrammarPack, dialect_for_path} is public.
        - ensure_snapshot(&ContextStore, &GraphStore, project_root: &Path, SnapshotPolicy) -> SnapshotOutcome (no Result; HEAD is outcome.revision). GraphStore::new(store.root(), work_dir.root()). The temp repo runs `git init`, `git config user.name t`, `git config user.email t@t` before its first commit.
        - loom::context::source_graph::{SourceEdge, SourceEdgeKind, EdgeProvenance, UNRESOLVED_TARGET, GRAPH_SCHEMA_VERSION}.
          EdgeProvenance variants: Structural, Compiler, Receiver, Import, LocalName, UniqueName, Syntax.
          SourceEdge fields: from, to, kind, provenance, confidence: f32, symbol, sites: Vec<Span>, candidates: Vec<String>, receiver: Option<String>.
          Span fields: start_byte, end_byte, line_start, line_end (1-based lines).
          SourceNode gains symbol_key: String (empty unless the id was disambiguated; otherwise the un-suffixed base id).
          A disambiguated id is "<base id>@<8 lowercase hex>" (optionally ".<n>").
        - loom::context::refresh::{ensure_snapshot, SnapshotPolicy, SnapshotAction}; loom::context::graph_store::{GraphStore, GraphLayer} with GraphLayer.schema_version: u32 and GraphStore::base_path(&self, revision: &str) -> PathBuf and load_base(&self, revision) -> Result<Option<GraphLayer>>; loom::context::store::ContextStore::open(&WorkDir) and .ensure() and .root(); loom::fs::work_dir::WorkDir::new(root) and .root(). Mirror commands/map.rs::load_graph for the store setup, in a temp git repo with one committed src/lib.rs.
        - Fixture paths are relative to the loom package root (cargo test cwd): tests/fixtures/source/{rust,typescript,python,go}.

        Settled rules the contracts pin (design 3, 4, 6.2):
        - A bare call whose spelling matches two or more same-file definitions and no lexical-scope winner is a Syntax edge to UNRESOLVED_TARGET, with sorted candidates.
        - Scope eligibility applies before counting: a single same-file match outside the caller's scope is never bound (Syntax, candidates == [that id]); a spelling that a named import binds is Syntax with no candidates, left to the resolver.
        - Two calls to one callee are ONE edge with two sites.
        - Only Contains edges may have confidence 1.0, and they have provenance Structural.
        - A base layer file that fails to parse is rebuilt (SnapshotAction::Rebuilt) and rewritten with GRAPH_SCHEMA_VERSION.

        EXPECTED INTEGRITY EVENTS: TI-edit on existing source-graph test files whose provenance and confidence assertions are rewritten per design 3.1, including the inline test asserts on EdgeProvenance::Inferred in extract/typescript.rs, extract/python.rs and extract/go.rs, and the map/views/json.rs key-set tests W5 extends. File ONE dispute-integrity listing every event after the final review round, reason quoting design 3.1. Never edit loom/maintainability-baseline.txt (ratchet): split files instead (design 15).
        CROSS-PLAN: W1's renames in loom/src/context/tests/{rank_source_expand,rank_source_expand_fusion,source_fixtures,retrieve_source,pack_required}.rs fall in PLAN-web-host-graft-followthrough.md territory; if that plan merged first, re-read them before W1 starts.
        MEMORY: record mistakes, decisions and surprises via loom memory immediately (subagents too); never loom knowledge in this stage; never Claude Code auto-memory.

        AMENDMENTS (pressure test; override the briefs and design.md; paste each worker's lines into its spawn prompt):
        - W1: rewrite, to `..Default::default()`, the full `ImpactOptions` literals in map/views/json.rs, map/views/mod.rs and verify/goal_backward/reachable.rs, and the two full `ResolutionStats` literals in map/views/tests.rs. M1 (stage 4) and R2 (stage 3) add fields to both types in files they do not own.
        - W1: extract/mod.rs declares `pub mod dialect;`. `Capabilities` derives Debug, Clone, Copy, Default, PartialEq, Eq, Serialize. Name the constants AMBIGUOUS_CANDIDATE_CONFIDENCE (0.2) and MAX_CANDIDATES (8) in source_graph/mod.rs. One `fn syntax_confidence(kind) -> f32` (0.3 for Calls, 0.5 otherwise) serves both SourceEdge::syntax callers and SourceEdge::unbind.
        - W1: `DialectSpec` gains two columns, both used at extraction and resolution: `self_receivers: &'static [&'static str]` (rust ["self","Self"]; typescript, tsx, javascript, java, csharp, cpp ["this"]; python ["self","cls"]; ruby ["self"]; php ["$this","self","static"]; go and c []) and `bare_calls_reach_members: bool` (true for java, csharp, cpp, ruby; false otherwise). `QueryHarness::self_receivers()` defaults to the dialect's list.
        - W1: a `Lookup::Gap` file node carries `parser_version = LEXICAL_PARSER_VERSION` and `language = dialect.language`. `layer_is_current`, `parser_version_matches` and the worktree_graph currency check treat Gap exactly like Unknown. Add a test: a base holding a gap-dialect file is Reused on the second ensure_snapshot.
        - W1: `ImportBinding::local_name()` returns None for glob and for `alias == Some("")`. Harnesses set `alias` explicitly whenever the bound local name is not the last path segment: Python `import a.b` gives alias Some("a.b"), so the receiver text `a.b` in `a.b.f()` matches it (resolver rule 2). A side-effect import (TS `import "x"`, a statement-level `require("x")`, Go `import _ "p"`) gives alias Some(""). Go `import . "p"` is glob.
        - W1: `refresh/tests_source_graph/layer_reuse.rs` holds `.supports(` calls; they are W1's. `verify/goal_backward/definition_sites.rs` has no `supports()` call: only the OnceLock registry change applies there.
        - W2: design 6.2 rules 3 and 4, for a spelling with no qualifier: when the file's dialect has `bare_calls_reach_members == false`, drop every id whose innermost enclosing definition is Type, Implementation or Interface before counting. Test: `impl W { fn parse(&self) {} fn load(&self) { parse(); } }` gives a Syntax edge to UNRESOLVED_TARGET with no candidates. Qualified spellings (`W::parse()`, `Self::parse()`) keep members.
        - W2: design 6.2 rules 3 and 4, scope eligibility BEFORE counting (this replaces the "exactly one id: bind" shortcut; `build.rs::spellings` exposes a bare `helper` for a nested `hidden::helper`, so a map hit alone proves nothing). Step 1: when the spelling's first segment equals the `local_name()` of a non-glob `ImportBinding` of the file, emit Syntax to UNRESOLVED_TARGET with NO candidates and stop; the resolver's rule 4 decides (the resolver never revisits an edge that has candidates). Step 2: an id is eligible when its anchor scope (the id's scope minus the spelling's segments; for a bare name, the parent scope) is a prefix of the caller's scope. Step 3: exactly one eligible id with the longest anchor binds LocalName; otherwise emit Syntax with ALL ids as candidates (one ineligible id gives candidates of length one, never a bind). `build.rs::call_edges` and `reference_edges` read the file's import bindings, so `import_bindings` runs before them. Tests in treesitter/tests.rs: the hidden-only and lexical-module cases of contract out-of-scope-or-imported-name-is-not-bound-locally, plus a qualified `W::parse()` from a top-level fn (anchor [] is eligible, binds LocalName). Rust import bindings are W3's, so W3 adds the imported-name case to extract/rust/tests.rs.
        - W2: design 6.2 rule 1, when no enclosing Type/Implementation/Interface exists and the enclosing function carries a `@definition.qualifier`, T is that function's scope minus its last segment (C++ `void W::run() { this->m(); }`).
        - W3: capture Rust `Self::helper()` as `@call.receiver` = Self plus `@call.name` = helper, `(call_expression function: (scoped_identifier path: (identifier) @call.receiver name: (identifier) @call.name) (#eq? @call.receiver "Self"))`, and exclude `Self` paths from the whole-path call pattern. Anchor every rust.rs edit on the pattern text, never on a line number. Python bindings follow the W1 local_name rule above. `use external_crate::helper;` yields a non-glob binding whose local_name() is helper; add the imported-name case of contract out-of-scope-or-imported-name-is-not-bound-locally to extract/rust/tests.rs.
        - W4: `commands/run/tests/preflight.rs::test_preflight_silent_when_base_exists` publishes `GraphLayer { revision, ..Default::default() }` (schema_version 0), which is never current after W4; set `schema_version: GRAPH_SCHEMA_VERSION` in that literal. Anchor `read_layer` by symbol: W1 moves it. The worktree_graph_tests.rs regression (a base entry with a different `parser_version` is re-extracted) is named `stale_parser_base_entry_is_reextracted`; stage resolved-view re-runs it by that name.
        - W5: `extractor` is design 8's wording ("registered", "pack {feature} not compiled", "no extractor"); `gaps[].detail` is design 5.3's detail text. A zero byte total renders the files-only share, with no division. The mixed-graph `.java` gap assertion is guarded at runtime on `!GrammarPack::WaveB.compiled()` and otherwise asserts `.java` reports "registered" (language-packs compiles wave B by default). W5 deletes loom/src/context/coverage.rs when it creates coverage/mod.rs, and owns map/views/mod.rs::render_footer.
      files:
        - "src/context/**"
        - "src/map/**"
        - "src/verify/**"
        - "src/commands/run/tests/preflight.rs"
        - "src/commands/hook/tests_user_prompt_e2e.rs"
        - "src/commands/hook/worker_brief/test_support.rs"
        - "src/commands/knowledge/tests_context.rs"
        - "src/commands/knowledge/bootstrap/tests_clusters.rs"
        - "src/orchestrator/signals/tests_brief_e2e.rs"
        - "src/plan/schema/tests/v2_lint_tests.rs"
        - "tests/integration/source_graph_fixtures.rs"
        - "tests/fixtures/source/**"
        - "tests/graph_contract_contracts.rs"
      before_stage:
        - command: "rg -n -F 'confidence: 1.0' src/context/source_graph/edge.rs"
          exit_code: 0
          stdout_contains: ["confidence: 1.0"]
          description: "BEFORE: the Parser constructor stamps same-file edges at confidence 1.0"
      after_stage:
        - command: "cargo test --test graph_contract_contracts only_containment_is_certain -- --exact"
          exit_code: 0
          stdout_contains: ["1 passed"]
          description: "AFTER: only structural containment edges reach confidence 1.0"
      acceptance:
        - "cargo build --all-targets"
        - "cargo clippy --all-targets -- -D warnings"
        - "cargo fmt --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps"
        - "cargo clippy --no-default-features --lib --bins -- -D warnings"
        - "cargo test --all-targets --no-fail-fast"
        - "cargo test --lib context::extract::treesitter::"
        - "cargo test --lib context::refresh::tests_schema"
        - "cargo test --lib context::graph_store::tests_corrupt"
        - "cargo test --lib context::worktree_graph::tests::stale_parser_base_entry_is_reextracted -- --exact"
        - "cargo test --lib context::coverage::"
        - "cargo test --test integration source_graph_fixtures"
        - "cargo test --test maintainability"
      artifacts:
        - "src/context/extract/dialect.rs"
        - "src/context/source_graph/imports.rs"
        - "src/context/extract/treesitter/ids.rs"
        - "src/context/coverage/dialects.rs"
      wiring:
        - source: "src/context/extract/mod.rs"
          pattern: "extractor_for\\("
          description: "extract_file looks extractors up through the dialect table"
        - source: "src/context/refresh/snapshot.rs"
          pattern: "GRAPH_SCHEMA_VERSION"
          description: "layer_is_current rejects layers from an older schema"
        - source: "src/context/extract/treesitter/build.rs"
          pattern: "disambiguate\\("
          description: "build applies id disambiguation before emitting nodes"
        - source: "src/map/views/json.rs"
          pattern: "by_dialect"
          description: "footer_json carries coverage by dialect"
        - source: "src/context/coverage/dialects.rs"
          pattern: "extractor_for\\("
          description: "coverage reports each dialect's extractor status through the same lookup"
      reachable:
        - symbol: extractor_for
          from: extract_file
          min_confidence: 0.5
          description: "extract_file reaches extractors only through the dialect lookup"
      contracts:
        - id: ambiguous-local-call-keeps-candidates
          file: tests/graph_contract_contracts.rs
          test: ambiguous_local_call_keeps_candidates
          scenario: "extracts src/lib.rs = 'mod a { pub fn helper() {} }\\nmod b { pub fn helper() {} }\\nfn run() { helper(); }\\n' with extract_file(&registry(), ..) and selects the Calls edge from src/lib.rs#function:run whose symbol is helper"
          rejects: "a build that keeps the last-spelling-wins map and emits a bound edge (local-name, or 1.0) to src/lib.rs#function:b::helper instead of a Syntax edge to UNRESOLVED_TARGET whose candidates are both helper ids"
        - id: repeated-calls-keep-every-site
          file: tests/graph_contract_contracts.rs
          test: repeated_calls_keep_every_site
          scenario: "extracts src/lib.rs = 'fn helper() {}\\nfn run() {\\n    helper();\\n    helper();\\n}\\n', collects the Calls edges from src/lib.rs#function:run with symbol helper, and asserts there is EXACTLY ONE, with sites line_start [3, 4], provenance LocalName and confidence 0.8"
          rejects: "a dedupe that collapses the two calls into one edge with fewer than two sites (line_start 3 and 4), that emits two edges each carrying both sites, or that keeps the edge at confidence 1.0 instead of LocalName 0.8"
        - id: duplicate-declarations-get-distinct-ids
          file: tests/graph_contract_contracts.rs
          test: duplicate_declarations_get_distinct_ids
          scenario: "extracts src/lib.rs with 'pub struct W;' and two blocks 'impl From<u8> for W { fn from(välue: u8) -> Self { W } }' and 'impl From<u16> for W { fn from(välue: u16) -> Self { W } }' (a multi-byte parameter name) and collects Function nodes whose scope is [W, from]; asserts two distinct ids, each matching ^src/lib.rs#function:W::from@[0-9a-f]{8}$ with symbol_key src/lib.rs#function:W::from; then re-extracts with the two impl blocks swapped and asserts the same id set"
          rejects: "an extractor that still emits two nodes sharing the id src/lib.rs#function:W::from, suffixed ids whose symbol_key is not that base id, or an ordinal or line-derived suffix that changes when the blocks swap (design 4: a signature hash; line numbers never enter an id)"
        - id: only-containment-is-certain
          file: tests/graph_contract_contracts.rs
          test: only_containment_is_certain
          scenario: "extracts every file under tests/fixtures/source/ recursively (relative to the loom package root) and inspects every edge, asserting at least 4 dialect directories were read and at least one Calls edge exists"
          rejects: "a harness that still stamps a same-file call edge at confidence 1.0; any edge at confidence >= 1.0 whose provenance is not Structural or whose kind is not Contains"
        - id: corrupt-base-layer-is-rebuilt
          file: tests/graph_contract_contracts.rs
          test: corrupt_base_layer_is_rebuilt
          scenario: "in a TempDir git repo with one committed src/lib.rs, runs ensure_snapshot(BaseOnly), overwrites GraphStore::base_path(HEAD) with the bytes 'not json', runs ensure_snapshot(BaseOnly) again, then load_base(HEAD)"
          rejects: "a snapshot that propagates the parse error as SnapshotAction::Unavailable (the wedge) or keeps the corrupt file, instead of returning Rebuilt and a layer with schema_version == GRAPH_SCHEMA_VERSION"
        - id: out-of-scope-or-imported-name-is-not-bound-locally
          file: tests/graph_contract_contracts.rs
          test: out_of_scope_or_imported_name_is_not_bound_locally
          scenario: "extracts three versions of src/lib.rs and inspects the Calls edge with symbol helper in each: (a) 'mod hidden {\\n    pub fn helper() {}\\n}\\nuse external_crate::helper;\\nfn run() {\\n    helper();\\n}\\n' from src/lib.rs#function:run expects provenance Syntax, to UNRESOLVED_TARGET, empty candidates; (b) 'mod hidden {\\n    pub fn helper() {}\\n}\\nfn run() {\\n    helper();\\n}\\n' from src/lib.rs#function:run expects Syntax, UNRESOLVED_TARGET, candidates == [src/lib.rs#function:hidden::helper]; (c) 'mod m {\\n    pub fn helper() {}\\n    pub fn run() {\\n        helper();\\n    }\\n}\\n' from src/lib.rs#function:m::run expects LocalName, to src/lib.rs#function:m::helper, confidence 0.8"
          rejects: "a single-match shortcut that binds hidden::helper with LocalName 0.8 from a caller outside hidden (cases a and b), one that ignores the same-name import or turns it into a final candidate set the resolver never revisits (case a), or an over-strict rule that refuses the lexically visible m::helper (case c)"

    - id: language-packs
      name: "TSX, JavaScript and grammar packs"
      summary: "Adds TSX, JavaScript/JSX, Java, C#, Ruby, PHP, C and C++ extractors behind default-on grammar-pack features, each reporting a named gap when its pack is not compiled."
      stage_type: standard
      skills: ["loom-rust"]
      subagent_timeout_secs: 900
      working_dir: "loom"
      dependencies: ["graph-contract"]
      description: |
        Implement design section 5.4 and the new dialects of 5.1 using the stage-1 harness (design 6).
        Use parallel subagents and skills to maximize performance.
        Spawn every worker BY AGENT TYPE with the fixed prompt plus "Your brief: <path>. Read it in full before anything else."
        Territories are DISJOINT within a wave. Workers NEVER spawn subagents.
        Waves: L0; then L1-L4 in ONE message.
        L0 creates the eight extractor module files; L1-L4 own them in wave 2.
        Before wave 1 and after wave 2, the main agent runs "cargo build --manifest-path loom/Cargo.toml --release" and records binary size and elapsed time with loom memory note (measurement, not a gate).

        | Worker | Role | Tier | Files owned | Shared context | Brief path |
        | ------ | ---- | ---- | ----------- | -------------- | ---------- |
        | L0 | Packs, deps, registry | sonnet | Cargo.toml; Cargo.lock; src/context/extract/mod.rs; src/context/extract/dialect.rs; src/context/extract/tests.rs | design.md 5 | doc/plans/briefs/source-graph-mechanism/language-packs/l0-packs-and-registry.md |
        | L1 | TSX and JavaScript | sonnet | src/context/extract/tsx.rs; src/context/extract/javascript.rs; src/context/extract/typescript.rs; tests/fixtures/source/tsx; tests/fixtures/source/javascript | design.md 3.4, 6 | doc/plans/briefs/source-graph-mechanism/language-packs/l1-tsx-javascript.md |
        | L2 | Java and C# | sonnet | src/context/extract/java.rs; src/context/extract/csharp.rs; tests/fixtures/source/java; tests/fixtures/source/csharp | design.md 3.4, 4, 6 | doc/plans/briefs/source-graph-mechanism/language-packs/l2-java-csharp.md |
        | L3 | Ruby and PHP | sonnet | src/context/extract/ruby.rs; src/context/extract/php.rs; tests/fixtures/source/ruby; tests/fixtures/source/php | design.md 3.4, 6 | doc/plans/briefs/source-graph-mechanism/language-packs/l3-ruby-php.md |
        | L4 | C and C++ | sonnet | src/context/extract/c.rs; src/context/extract/cpp.rs; tests/fixtures/source/c; tests/fixtures/source/cpp | design.md 3.4, 5.1, 6 | doc/plans/briefs/source-graph-mechanism/language-packs/l4-c-cpp.md |

        Pinned crates (cargo add <crate>@=<version> --optional): tree-sitter-javascript 0.25.0, tree-sitter-java 0.23.5, tree-sitter-c-sharp 0.23.5, tree-sitter-ruby 0.23.1, tree-sitter-php 0.24.2, tree-sitter-c 0.24.2, tree-sitter-cpp 0.23.4.
        Features: default = [source-graph, source-graph-wave-b, source-graph-wave-c]; source-graph adds dep:tree-sitter-javascript; wave-b = [source-graph, java, c-sharp, ruby, php]; wave-c = [source-graph, c, cpp].
        The [features] block is the one hand edit: no cargo command manages it.
        Extractor struct names: TsxExtractor, JavaScriptExtractor, JavaExtractor, CSharpExtractor, RubyExtractor, PhpExtractor, CExtractor, CppExtractor.
        Run every acceptance command once in-session before loom stage complete, so the --no-default-features builds are warm (each acceptance command has a 300 s cap).

        CONTRACT SURFACE (loom/tests/language_packs_contracts.rs):
        - extract_file(&registry(), path, bytes) as in stage graph-contract.
        - loom::context::source_graph::NodeLanguage::{Tsx, JavaScript, Java, CSharp, Ruby, Php, C, Cpp}; SourceEdgeKind::References; FileExtraction.imports: Vec<ImportBinding { path, name: Option<String>, alias: Option<String>, glob: bool, site }>; SourceNode.scope, .language, .coverage, .symbol_key.
        - JSX: a capitalized element name (<Button .../>) in a .tsx or .jsx file is a References edge from the enclosing function with symbol "Button" and a site on the element's line.
        - CommonJS: 'const util = require("./util")' yields an Imports edge with symbol "./util" and an ImportBinding with path "./util" and alias Some("util").
        - .jsx belongs to the javascript dialect; .tsx alone uses the TSX grammar.

        EXPECTED INTEGRITY EVENTS: TI-edit for removing the stage-1 ".tsx is a gap" assertion in loom/src/context/extract/tests.rs; dispute it once with reason "TSX is now registered (design 5.3)".
        MEMORY: record mistakes, decisions and surprises via loom memory immediately (subagents too), including the release size and build-time measurements; never loom knowledge; never auto-memory.

        AMENDMENTS (pressure test; override the briefs and design.md; paste each worker's lines into its spawn prompt):
        - ALL (L0-L4): `@definition.*` always goes on the whole declaration node (function_definition, method_declaration, class_specifier, ...), never on a declarator. The C and C++ tags.scm files put @definition.function on function_declarator: copying them would make prototypes definitions and shrink the span to `run()`, so calls in a body would be attributed to the enclosing class or file. Spans drive scope and call attribution. A declaration without a body (a prototype, an in-class `void run();`) is not a definition.
        - ALL: every new extractor starts at extractor_version 1 and honours its dialect's `self_receivers` and `bare_calls_reach_members` columns (graph-contract W1). PHP node types live at php/src/node-types.json in the tree-sitter-php crate, not src/node-types.json.
        - ALL: a namespace or package Module node's scope is ONE segment holding the dotted name: Java `package a.b;` gives ["a.b"], C# `namespace A.B` (block or file-scoped) gives ["A.B"], PHP `namespace A\B;` gives ["A.B"]. binding-resolver's namespace index reads that segment; each worker's tests assert the shape.
        - Main agent: the release measurement runs in the background (Bash run_in_background; a foreground call dies at the tool timeout). Record `stat -c %s target/release/loom` and `cargo build --release --timings` per-unit durations of the seven grammar crates, before wave 1 and after wave 2. The "before" build is cold in a fresh worktree and the "after" build reuses release deps, so compare unit durations and binary size, not wall time. Measured on the host: the binary grows from about 39 MB to about 55 MB; gcc compiles the C# parser.c in about 1.7 s.
        - L0: the uncompiled-pack gap test is named `uncompiled_wave_b_pack_is_a_named_gap`, lives in extract/tests.rs under `#[cfg(not(feature = "source-graph-wave-b"))]`, and asserts the design 5.3 detail text for a `.java` file. Remove the per-dependency feature lines `cargo add --optional` writes into [features] (for example `tree-sitter-java = ["dep:tree-sitter-java"]`); the pack features replace them.
        - L1: CommonJS uses two disjoint patterns, `(variable_declarator value: (call_expression function: (identifier) @_req (#eq? @_req "require") arguments: (arguments (string) @import.path))) @import.statement` and `(expression_statement (call_expression function: (identifier) @_req (#eq? @_req "require") arguments: (arguments (string) @import.path)) @import.statement)`. The binding site is the @import.path span. The plain-call pattern carries `(#not-eq? @call.name "require")`, so require is never a Calls edge. A statement-level require is a side-effect import (alias Some("")).
        - L2: C# generic calls `M<T>()` and `obj.M<T>()` have a `generic_name` name/function; capture its identifier. In `using_directive`, the alias is the `name:` field and the path is the unnamed type child; capture that child as @import.path, never any identifier.
        - L3: PHP `A::m()` emits `@call.name` = m and `@call.receiver` = A, with no "A::m" symbol; the resolver treats A under rule 2 (bound through `use X\A;` when A is an import's local name, otherwise candidates only).
        - L3 (import spec, D-import-spec; binding-resolver's R1 reads the same encoding, so never change it in-stage): capture the method name (`@_m`) beside `@import.path`. `require_relative 'x'` gives path `./x`; a spec already starting `./` or `../` is kept as written, so `require_relative '../lib/x'` stays `../lib/x`. `require 'x'` and `load 'x'` keep the spec verbatim. All three are one glob binding. Test `require_relative_and_require_keep_distinct_specs` in extract/ruby.rs's `#[cfg(test)] mod tests` (acceptance runs it with --exact): `require_relative 'x'` and `require 'x'` in one file yield paths `./x` and `x`.
        - L4: an out-of-line `void W::run() {}` captures W as `@definition.qualifier`, so its scope is [W, run]; `this->m()` inside it binds through graph-contract's W2 rule-1 amendment. `#include "x.h"` is a whole-file binding (glob true, path `x.h`); `#include <x>` is external, encoded as path `<x>` with the leading `<` kept (D-import-spec).
      files:
        - "Cargo.toml"
        - "Cargo.lock"
        - "src/context/extract/**"
        - "src/context/coverage/**"
        - "tests/fixtures/source/**"
        - "tests/language_packs_contracts.rs"
      before_stage:
        - command: "rg -q -F 'tree-sitter-javascript' Cargo.toml"
          exit_code: 1
          description: "BEFORE: no JavaScript grammar is a dependency"
      after_stage:
        - command: "cargo test --test language_packs_contracts wave_pack_dialects_extract_declarations -- --exact"
          exit_code: 0
          stdout_contains: ["1 passed"]
          description: "AFTER: every wave-B and wave-C dialect extracts declarations"
      acceptance:
        - "cargo build --all-targets"
        - "cargo clippy --all-targets -- -D warnings"
        - "cargo fmt --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps"
        - "cargo clippy --no-default-features --lib --bins -- -D warnings"
        - "cargo clippy --no-default-features --features source-graph --lib --bins -- -D warnings"
        - "cargo clippy --no-default-features --features source-graph-wave-b --lib --bins -- -D warnings"
        - "cargo clippy --no-default-features --features source-graph-wave-c --lib --bins -- -D warnings"
        - "cargo test --no-default-features --features source-graph --lib context::extract::"
        - "cargo test --no-default-features --features source-graph --lib context::extract::tests::uncompiled_wave_b_pack_is_a_named_gap -- --exact"
        - "cargo test --all-targets --no-fail-fast"
        - "cargo test --lib context::extract::ruby::tests::require_relative_and_require_keep_distinct_specs -- --exact"
        - "cargo audit --no-fetch"
        - "cargo test --test graph_contract_contracts"
        - "cargo test --test integration source_graph_fixtures"
        - "cargo test --test maintainability"
      artifacts:
        - "src/context/extract/tsx.rs"
        - "src/context/extract/javascript.rs"
        - "src/context/extract/java.rs"
        - "src/context/extract/csharp.rs"
        - "src/context/extract/ruby.rs"
        - "src/context/extract/php.rs"
        - "src/context/extract/c.rs"
        - "src/context/extract/cpp.rs"
      wiring:
        - source: "src/context/extract/mod.rs"
          pattern: "JavaExtractor::new\\(\\)"
          description: "registry() registers the Java extractor"
        - source: "src/context/extract/mod.rs"
          pattern: "CppExtractor::new\\(\\)"
          description: "registry() registers the C++ extractor"
        - source: "src/context/extract/mod.rs"
          pattern: "TsxExtractor::new\\(\\)"
          description: "registry() registers the TSX extractor"
        - source: "Cargo.toml"
          pattern: "source-graph-wave-b"
          description: "grammar pack feature declared"
      contracts:
        - id: tsx-file-yields-symbol-nodes
          file: tests/language_packs_contracts.rs
          test: tsx_file_yields_symbol_nodes
          scenario: "extracts src/App.tsx = 'import { Button } from \"./Button\";\\nexport function App() {\\n  return <Button label=\"x\" />;\\n}\\n' and looks for node src/App.tsx#function:App"
          rejects: "routing .tsx to the lexical fallback (a file node with FileCoverage::LexicalOnly), or tagging it NodeLanguage::TypeScript instead of Tsx with FileCoverage::Full"
        - id: jsx-element-becomes-reference-edge
          file: tests/language_packs_contracts.rs
          test: jsx_element_becomes_reference_edge
          scenario: "extracts src/App.jsx = 'import { Button } from \"./Button\";\\nexport function App() {\\n  return <Button label=\"x\" />;\\n}\\n' and looks for a References edge from src/App.jsx#function:App; then extracts the same source as src/App.tsx and looks for the same edge from src/App.tsx#function:App"
          rejects: "a JavaScript or TSX query that parses JSX but never emits component usages (no References edge with symbol Button and a site on line 3)"
        - id: commonjs-require-is-an-import-binding
          file: tests/language_packs_contracts.rs
          test: commonjs_require_is_an_import_binding
          scenario: "extracts src/main.js = 'const util = require(\"./util\");\\nfunction run() {\\n  util.parse();\\n}\\n' and inspects FileExtraction.imports and the Imports edges"
          rejects: "treating require as a plain call: no Imports edge with symbol ./util and no ImportBinding with path ./util and alias Some(util)"
        - id: wave-pack-dialects-extract-declarations
          file: tests/language_packs_contracts.rs
          test: wave_pack_dialects_extract_declarations
          scenario: "extracts src/a/Widget.java 'package a;\\npublic class Widget {\\n  public void run() {}\\n}\\n', src/Widget.cs 'namespace A {\\n  public class Widget {\\n    public void Run() {}\\n  }\\n}\\n', lib/widget.rb 'class Widget\\n  def run\\n  end\\nend\\n', src/Widget.php '<?php\\nclass Widget {\\n  public function run() {}\\n}\\n', src/run.c 'int run(void) { return 0; }\\n', src/widget.cpp 'class Widget {\\n public:\\n  void run() {}\\n};\\n' and, per file, finds a Function node whose scope ends with [Widget, run] ([Widget, Run] for C#, [run] for C)"
          rejects: "a registry that leaves any wave-B or wave-C dialect on the lexical fallback, or tags its nodes with the wrong NodeLanguage, or reports coverage other than FileCoverage::Full"
        - id: java-overloads-get-distinct-ids
          file: tests/language_packs_contracts.rs
          test: java_overloads_get_distinct_ids
          scenario: "extracts src/W.java = 'class W {\\n  void f(int a) {}\\n  void f(String s) {}\\n}\\n' and collects Function nodes whose scope ends with [W, f]"
          rejects: "a Java query whose two overloads collapse onto one id (two nodes, one id) or lack a shared non-empty symbol_key"

    - id: binding-resolver
      name: "Language-aware resolution"
      summary: "Cross-file resolution follows imports, aliases, package scope and receivers per language, refuses name-only binds it cannot justify, keeps ambiguity as candidate sets, and records the lookups each edge depended on."
      stage_type: standard
      skills: ["loom-rust"]
      subagent_timeout_secs: 900
      working_dir: "loom"
      dependencies: ["graph-contract"]
      description: |
        Implement design section 7 (7.0 recording API, rules 1-7, 7.1 path conventions) of doc/plans/briefs/source-graph-mechanism/design.md.
        Use parallel subagents and skills to maximize performance.
        Spawn every worker BY AGENT TYPE with the fixed prompt plus "Your brief: <path>. Read it in full before anything else."
        Waves: R1; then R2. Workers NEVER spawn subagents.

        | Worker | Role | Tier | Files owned | Shared context | Brief path |
        | ------ | ---- | ---- | ----------- | -------------- | ---------- |
        | R1 | Path conventions | sonnet | src/context/resolve/paths/mod.rs; src/context/resolve/tests_paths.rs | design.md 5.1, 7.1 | doc/plans/briefs/source-graph-mechanism/binding-resolver/r1-path-conventions.md |
        | R2 | Resolution rules | opus | src/context/resolve.rs; src/context/resolve/rules.rs; src/context/resolve/bindings.rs; src/context/resolve/receivers.rs; src/context/resolve/record.rs; src/context/resolve/symbols.rs; src/context/resolve/tests_rules.rs; src/context/resolve/tests_recording.rs | design.md 3, 7, 12 | doc/plans/briefs/source-graph-mechanism/binding-resolver/r2-resolution-rules.md |

        R2 also updates assertions in resolve/tests_resolve.rs, tests_qualified.rs and fixtures.rs where a rule changes an outcome. Those files are its own after R1 finishes.

        CONTRACT SURFACE (loom/tests/binding_resolver_contracts.rs):
        - Build graphs in memory: for each (path, source) pair, let extraction = extract_file(&registry(), Path::new(path), bytes); FileEntry::from_extraction(bytes, extraction); collect into ResolvedGraph { base_revision: String::new(), overlaid: BTreeSet::new(), files: BTreeMap<String, FileEntry> } keyed by path; then loom::context::resolve_graph(&mut graph).
        - Inspect the edge by (from node id, symbol): its to, provenance (EdgeProvenance::{Import, Receiver, UniqueName, Syntax}), candidates and receiver.
        - Rules pinned:
          - an import alias binds the aliased definition with Import;
          - a self./this. call whose type's member lives in another file of the same family binds with Receiver;
          - a member call on any other receiver is never bound by name (it stays Syntax; one same-family definition of that name becomes candidates == [that id]);
          - a glob import whose module resolves to no file refuses UniqueName;
          - names never bind across families (python vs go).

        EXPECTED INTEGRITY EVENTS: TI-edit in resolve/tests_resolve.rs and tests_qualified.rs where a rule changes an outcome; R2 lists each. One is known now: tests_resolve.rs (the `./language` import from src/app.ts that expects `stats.ambiguous == 1` between src/a/language.ts and src/b/language.ts) matches nothing under design 7.1's relative resolution. File ONE dispute-integrity after the final review round.
        MEMORY: record mistakes, decisions and surprises via loom memory immediately (subagents too); never loom knowledge; never auto-memory.

        AMENDMENTS (pressure test; override the briefs and design.md; paste each worker's lines into its spawn prompt):
        - R1: every path-convention function takes `keys: &mut BTreeSet<String>` and records `ns:{family}:{namespace}` whenever it consults the namespace index, plus `pathset:{family}` for every module-path or package-scope lookup. `module_files` returns only files whose dialect is in the importer's family, so a verbatim `./styles.css` probe never matches.
        - R1 (rust): a non-anchored path (`x::y`, no `crate::`/`self::`/`super::`) is internal only when its first segment names a module file or directory under a crate root (a directory holding lib.rs or main.rs) or under the citing module's directory; otherwise `module_files` returns empty and the import is external. The current `strip_foreign_root` plus suffix truncation turns `use serde::de::*` into `de` and matches any local de.rs. Add the negative test: `use serde::de::*` with src/x/de.rs present returns empty.
        - R1 (ruby; the import spec is language-packs L3's encoding, D-import-spec, and `module_files` keeps its signature): a spec starting `./` or `../` (written by L3 for `require_relative`) resolves against the importing file's directory only, `<spec>.rb`. A bare spec (from `require` or `load`) resolves to `lib/<spec>.rb` when that file exists, else to every `<spec>.rb` suffix match. c/cpp: a spec starting `<` is external (empty). Test `ruby_require_and_require_relative_pick_different_files` in tests_paths.rs: with app/x.rb and lib/x.rb both present and `from` = app/main.rb, `./x` gives [app/x.rb] and `x` gives [lib/x.rb].
        - R1: resolve/paths.rs exists at HEAD (282 lines); R1 moves it to resolve/paths/mod.rs (git mv) so the two never coexist, and owns the `mod paths;` line in resolve.rs. R1 leaves the tests_resolve.rs `./language` assertion failing and reports it; R2 updates it (the TI event above).
        - R1/R2: binding-resolver runs beside language-packs, so the Java, C#, Ruby, PHP, C and TSX extractors do not exist in this worktree. Tests for those dialects build FileEntry values by hand, using language-packs' Module-scope shape: one segment holding the dotted name (Java ["a.b"], C# ["A.B"], PHP ["A.B"]).
        - R2 step 0: split resolve/tests_resolve.rs (398 lines, 8 ResolutionStats literals) into a second tests_*.rs file BEFORE editing it. The maintainability baseline is an exact-count ratchet.
        - R2: `ResolutionStats` drops `Copy`, derives Serialize and Deserialize, and gains `by_provenance: BTreeMap<String, usize>` (owned keys: `&'static str` keys cannot deserialize). `EdgeRef` derives Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize. `resolve_graph` returns `resolution_stats(graph)` computed after resolving, so the two are equal by construction; `retargeted` counts edges whose provenance is Import, Receiver or UniqueName (extraction-time Receiver binds included), documented on the field.
        - R2: resolution never touches an unresolved Syntax edge whose `candidates` is non-empty on entry. Same-file ambiguity found at extraction (design 6.2) is final, records no keys, and is never unbound by relink. Otherwise relink (which unbinds and clears candidates) and a cold build would differ whenever a name has more than 8 definitions graph-wide.
        - R2: every name lookup records the key `name:{family}:{last "::" or "." segment of the looked-up spelling}`. One shared function in record.rs computes the keys a node answers to; `SymbolIndex::build` and `touched_keys` both call it, for every node kind the index holds (File nodes by file name and stem, Implementation nodes included). Add a tests_recording test that every SymbolIndex bucket maps to a key `touched_keys` emits for the owning entry.
        - R2: rule 3's self receivers come from the from-node's dialect (`dialect_for_path(path).self_receivers`). T is the innermost proper prefix of the from-node's scope that names a Type, Implementation or Interface node in the same file; match candidates on that node's own last name segment, so `impl Widget` at top level in another file matches `example::Widget` in an inline module.
        - R2: a refused edge (rules 4 and 6 refusing rule 7) still records 1-8 same-family candidates named like the call, as rule 2 does, so impact recall survives an external glob. Keep `node_names(&SourceNode) -> Vec<String>`: map/views/mod.rs and verify/goal_backward/reachable.rs use it.
        - R2: bumping RESOLVER_VERSION is the rule for any later change to rules 1-7 (the constant lands in stage resolved-view).
        - R2 writes three tests in tests_recording.rs under these exact names, which acceptance runs with --exact: `index_buckets_map_to_touched_keys`, `resolve_edges_matches_full_resolution_on_selected_edges` (unbind a chosen edge set, resolve_edges, compare with a full resolve_graph) and `stats_equal_resolution_stats_after_resolving`. A wiring regex cannot prove these APIs: a pattern that matches only a definition line is excluded (verify/goal_backward/wiring_v2.rs::file_match).
      files:
        - "src/context/resolve.rs"
        - "src/context/resolve/**"
        - "tests/binding_resolver_contracts.rs"
      before_stage:
        - command: "rg -q -F 'module_files' src/context/resolve"
          exit_code: 1
          description: "BEFORE: no per-dialect module resolution exists"
      after_stage:
        - command: "cargo test --test binding_resolver_contracts dynamic_receiver_call_is_not_bound_by_name -- --exact"
          exit_code: 0
          stdout_contains: ["1 passed"]
          description: "AFTER: dynamic receivers are never bound by name"
      acceptance:
        - "cargo build --all-targets"
        - "cargo clippy --all-targets -- -D warnings"
        - "cargo fmt --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps"
        - "cargo test --all-targets --no-fail-fast"
        - "cargo test --lib context::resolve::tests_recording::index_buckets_map_to_touched_keys -- --exact"
        - "cargo test --lib context::resolve::tests_recording::resolve_edges_matches_full_resolution_on_selected_edges -- --exact"
        - "cargo test --lib context::resolve::tests_recording::stats_equal_resolution_stats_after_resolving -- --exact"
        - "cargo test --lib context::resolve::tests_rules"
        - "cargo test --lib context::resolve::tests_paths::ruby_require_and_require_relative_pick_different_files -- --exact"
        - "cargo test --test graph_contract_contracts"
        - "cargo test --test integration source_graph_fixtures"
        - "cargo test --test maintainability"
      artifacts:
        - "src/context/resolve/rules.rs"
        - "src/context/resolve/record.rs"
      wiring:
        - source: "src/context/resolve/rules.rs"
          pattern: "module_files\\("
          description: "rules resolve modules through the per-dialect path conventions"
      contracts:
        - id: import-alias-binds-the-aliased-definition
          file: tests/binding_resolver_contracts.rs
          test: import_alias_binds_the_aliased_definition
          scenario: "resolves src/util.ts 'export function parse() {}\\n', src/other.ts 'export function parse() {}\\n', src/main.ts 'import { parse as p } from \"./util\";\\nexport function run() {\\n  p();\\n}\\n' and inspects the Calls edge from src/main.ts#function:run with symbol p"
          rejects: "a resolver that looks up the written name p (no definition) or the bare name parse (two definitions) instead of following the alias to src/util.ts#function:parse with provenance Import"
        - id: self-receiver-binds-across-files
          file: tests/binding_resolver_contracts.rs
          test: self_receiver_binds_across_files
          scenario: "resolves src/w.rs 'pub struct W;\\nimpl W {\\n    pub fn a(&self) {\\n        self.b();\\n    }\\n}\\n' and src/w_ext.rs 'impl W {\\n    pub fn b(&self) {}\\n}\\n' and inspects the Calls edge from src/w.rs#function:W::a with symbol b"
          rejects: "a resolver that leaves self.b() unresolved or binds it by graph-wide name uniqueness (UniqueName) instead of Receiver to src/w_ext.rs#function:W::b"
        - id: dynamic-receiver-call-is-not-bound-by-name
          file: tests/binding_resolver_contracts.rs
          test: dynamic_receiver_call_is_not_bound_by_name
          scenario: "resolves src/a.py 'class A:\\n    def save(self):\\n        pass\\n' and src/b.py 'def run(obj):\\n    obj.save()\\n' and inspects the Calls edge from src/b.py#function:run with symbol save"
          rejects: "binding obj.save() to src/a.py#function:A::save at unique-name confidence; the edge must stay Syntax, to UNRESOLVED_TARGET, with candidates == [src/a.py#function:A::save]"
        - id: external-glob-import-refuses-unique-name
          file: tests/binding_resolver_contracts.rs
          test: external_glob_import_refuses_unique_name
          scenario: "resolves src/a.rs 'use external_crate::*;\\npub fn run() {\\n    helper();\\n}\\n' and src/b.rs 'pub fn helper() {}\\n' and inspects the Calls edge from src/a.rs#function:run with symbol helper: provenance Syntax, to UNRESOLVED_TARGET, candidates == [src/b.rs#function:helper]"
          rejects: "a resolver that binds helper to src/b.rs#function:helper by graph-wide uniqueness although a glob import from a module outside the graph could supply it, or that drops the candidate and loses impact recall"
        - id: names-never-bind-across-families
          file: tests/binding_resolver_contracts.rs
          test: names_never_bind_across_families
          scenario: "resolves src/p.py 'def run():\\n    helper()\\n' and src/g.go 'package g\\nfunc helper() {}\\n' and inspects the Calls edge from src/p.py#function:run with symbol helper: to == UNRESOLVED_TARGET and candidates is empty"
          rejects: "a SymbolIndex keyed by name alone that binds the Python call to src/g.go#function:helper, or lists it as a candidate (impact follows candidates)"

    - id: map-api-freshness
      name: "Map API, freshness states, census and windows"
      summary: "loom map gains honest direct/transitive views with call sites, labelled substring fallback, snapshot identity in JSON, a portfolio census, source windows and timings; never-built, stale and unavailable graphs are labelled everywhere and never trigger a rebuild loop."
      stage_type: standard
      skills: ["loom-rust"]
      subagent_timeout_secs: 900
      working_dir: "loom"
      dependencies: ["binding-resolver"]
      description: |
        Implement design sections 9, 10, 10.1 and 11 of doc/plans/briefs/source-graph-mechanism/design.md.
        Use parallel subagents and skills to maximize performance.
        Spawn every worker BY AGENT TYPE with the fixed prompt plus "Your brief: <path>. Read it in full before anything else."
        Waves: M1, M2, M3 in ONE message; then M4. Territories are DISJOINT. Workers NEVER spawn subagents.
        graph-contract W1 already rewrote every full ImpactOptions literal (map/views/json.rs, map/views/mod.rs, verify/goal_backward/reachable.rs) to `..Default::default()`, so M1's new fields compile in wave 1. If `cargo build --all-targets` still names an ImpactOptions literal after M1, stop and brief a fix; do not let M2/M3 run against a broken lib.

        | Worker | Role | Tier | Files owned | Shared context | Brief path |
        | ------ | ---- | ---- | ----------- | -------------- | ---------- |
        | M1 | Query surface | sonnet | src/context/resolve/neighbors.rs; src/context/resolve/impact.rs; src/context/resolve/tests_neighbors.rs; src/context/resolve/tests_impact.rs; src/context/resolve/tests_candidates.rs; ../skills/loom-plan-writer/references/v2-contracts.md | design.md 3, 10 | doc/plans/briefs/source-graph-mechanism/map-api-freshness/m1-query-surface.md |
        | M2 | Freshness and lease | sonnet | src/context/freshness.rs; src/context/refresh.rs; src/context/refresh/snapshot.rs; src/context/refresh/snapshot/describe.rs; src/context/refresh/tests_states.rs; src/context/refresh/tests_snapshot.rs; src/context/refresh/tests_freshness.rs; src/context/graph_store/fallback.rs; src/context/tests/store.rs; src/context/tests/retrieve_source.rs; src/orchestrator/signals/format/brief.rs; src/orchestrator/signals/format/brief_tests.rs; src/commands/knowledge/context.rs; src/commands/knowledge/bootstrap/tests.rs; src/context/retrieve/graph.rs; src/commands/hook/reconcile_graph.rs; src/commands/hook/reconcile_graph/lock.rs; src/commands/hook/reconcile_graph/tests_wants_rebuild.rs; src/commands/hook/tests_reconcile_graph.rs; src/cli/types_ops.rs; src/cli/dispatch*.rs | design.md 9 | doc/plans/briefs/source-graph-mechanism/map-api-freshness/m2-freshness-lease.md |
        | M3 | Census and windows | sonnet | src/context/census/mod.rs; src/context/census/classify.rs; src/context/census/subprojects.rs; src/context/census/report.rs; src/context/census/tests.rs; src/context/window.rs; src/context/window_tests.rs; src/context/mod.rs; src/context/refresh/source_graph.rs | design.md 10.1, 11 | doc/plans/briefs/source-graph-mechanism/map-api-freshness/m3-census-window.md |
        | M4 | loom map surface | sonnet | src/commands/map.rs; src/commands/tests_map.rs; src/map/views/mod.rs; src/map/views/json.rs; src/map/views/tests.rs; tests/map_cli.rs | design.md 8, 10 | doc/plans/briefs/source-graph-mechanism/map-api-freshness/m4-map-cli-views.md |

        M4 also owns every new file under loom/src/map/views/ it creates while splitting views/mod.rs, and, after M2 finishes, the one MapArgs literal in loom/src/context/refresh/tests_snapshot.rs. M3 changes EXCLUDED_ROOTS/excluded in loom/src/context/refresh/source_graph.rs to pub(crate) (one line each). `mod source_graph;` is private in refresh.rs, so M2 (who owns refresh.rs in wave 1) adds `excluded, EXCLUDED_ROOTS` to the existing `pub(crate) use source_graph::{...}` line there; M3 imports them as `crate::context::refresh::{excluded, EXCLUDED_ROOTS}`.

        CONTRACT SURFACE (loom/tests/map_api_contracts.rs; binary tests run with cwd set to a TempDir git repo with committed files and user.name/user.email configured):
        - Spawn the binary ONLY through `helpers::loom_cmd()`, declared as `#[path = "integration/helpers.rs"] #[allow(dead_code)] mod helpers;` exactly as loom/tests/token_optimization_contracts.rs does. `tests/integration/binary_spawn_guard.rs` fails any other file that writes `Command::new(env!("CARGO_BIN_EXE_loom"))`, and an unscrubbed spawn inherits the caller's LOOM_* identity and real HOME.
        - "loom map --callers target --json" on src/lib.rs 'pub fn target() {}\npub fn caller() {\n    let _x = 1;\n    target();\n}\n' prints one JSON object. views.callers.neighbors[0] has id "src/lib.rs#function:caller", line_start 2 (its declaration line), and sites[0].line_start 4 (the call line).
        - "loom map --find-all target --json" top level has "schema": "loom-map/2" and a "snapshot" object with "state": "current", "base_revision" equal to the repo HEAD sha, and "schema_version": 2.
        - "loom map --impact targ --json" has views.impact.match == "substring".
        - "loom map --census --json" has top-level totals.{eligible,excluded,vendored,generated,unsupported}, each {files, bytes}. Classification order: excluded, vendored (path segment vendor/third_party/third-party/node_modules/bower_components/Pods or linguist-vendored), generated (linguist-generated or '@generated'/'DO NOT EDIT'/'Code generated'/'autogenerated' in the first 1024 bytes), eligible (extension has a dialect), unsupported.
        - "loom map --window <id>" with an id naming no graph node or file (for example '../outside.txt@0-5') exits 2, prints "unknown id" on stderr, and never prints the outside file's bytes. The contract uses an outer TempDir holding repo/ and outside.txt, so nothing is written to the shared /tmp root.
        - loom::commands::hook::reconcile_graph::wants_rebuild(freshness: &loom::context::freshness::Freshness, degraded: Option<&str>) -> bool follows design 9's truth table: false for Freshness::never_built("x") with or without degraded; false for Freshness::unavailable("x") with or without degraded; true for Freshness { revision: "abc".into(), stale: true, ..Default::default() }; true for a current Freshness (revision "abc", stale false) with Some("degraded"); false for that current Freshness with None.
        - Snapshot schema_version in JSON is compared with loom::context::source_graph::GRAPH_SCHEMA_VERSION, never the literal 2.

        EXPECTED INTEGRITY EVENTS: TI-edit in commands/tests_map.rs (require_view message), the map/views/json.rs key-set tests, resolve/tests_neighbors.rs (callers are calls-only), the brief/freshness word tests, and context/tests/retrieve_source.rs `retrieve_for_stage_is_not_degraded_when_the_semantic_layer_was_never_built` (a never-built, empty graph is now degraded, design 9). File ONE dispute-integrity after the final review round.
        CROSS-PLAN: cli/dispatch.rs is also owned by PLAN-model-router-hooks.md, and all of loom/src/cli/** by PLAN-web-host-graft-followthrough.md, whose pending worker C splits dispatch.rs into dispatch_commands.rs. Touch only the arm that dispatches HookCommands::ReconcileGraph; find it with `rg -n 'HookCommands::ReconcileGraph' loom/src/cli`.
        MEMORY: record mistakes, decisions and surprises via loom memory immediately (subagents too); never loom knowledge; never auto-memory.

        AMENDMENTS (pressure test; override the briefs and design.md; paste each worker's lines into its spawn prompt):
        - M1: ImpactOptions keeps `limit` and `path_prefix` with today's post-traversal semantics (impact.rs, after `walk.finish()`); ImpactResult gains `filtered_out: usize`, the count path_prefix removed, which M4 renders as design 10's `filters.path.filtered_out`. In v2-contracts.md replace the sentence beginning "The walk follows resolved edges only" (the before/after checks pin it); the new text says a candidate step trusts 0.2 and that `min_confidence` above 0.2 requires bound edges throughout (integration-verify checks the phrase "above 0.2").
        - M2 owns every construction site its new fields break: the full `Freshness` literals in context/freshness.rs, context/refresh.rs (three) and context/tests/store.rs; the full `SnapshotOutcome` literals in refresh/snapshot.rs, refresh/tests_snapshot.rs and commands/knowledge/bootstrap/tests.rs (`fn snapshot`). SnapshotOutcome has no Default; add the two fields at each site.
        - M2: a failed build keeps `action: SnapshotAction::Unavailable` whether or not it serves a stale base (`serving` distinguishes the two), so bootstrap's `require_snapshot` (commands/knowledge/bootstrap/graph.rs, `action == Unavailable`) still refuses it; its refusal test in bootstrap/tests.rs keeps its assertions.
        - M2: `Freshness.unavailable` is `#[serde(skip)]`, never persisted. Its producer is `refresh::semantic_freshness_against_head`: when `source_graph::head_revision(project_root)` returns None it returns `Freshness::unavailable(detail)` (revision kept) instead of the stored value.
        - M2: `degraded_reason` keeps its signature. The never-built reason fires only when `semantic_revision` is empty AND `graph.files.is_empty()`, with the fixed text "source graph never built; run loom map to build it". An overlay-backed read with an empty semantic revision is not degraded.
        - M2: `wants_rebuild` truth table: Stale gives true; Current gives true exactly when `degraded` is Some; NeverBuilt and Unavailable give false whatever `degraded` holds. `spawn_if_needed` calls it as `wants_rebuild(&pack.semantic_freshness, pack.degraded.as_deref())` and nothing else decides the spawn.
        - M2: every lock write goes through `fs::locking::locked_write` (directory lock plus atomic rename), never through today's `claim_lock` remove + create_new + write_all: between those steps a reader sees no file or an empty one, `decide` returns Spawn, and a second reconcile runs beside the live holder. Every write is read-modify-write under that one lock and carries `failures` and `pending` forward; `mark_pending` keeps the holder's epoch and pid. Test `pending_is_never_lost_across_threads` (in tests_wants_rebuild.rs): two threads, one marking pending while the other finishes; pending survives and no reader sees an empty or partial lock.
        - M2: `--cancel` signals a holder pid only when its NUL-split argv (from /proc/<pid>/cmdline, or `ps -p <pid> -o command=`) contains both `hook` and `reconcile-graph`, and only when the process started no later than the lock's epoch; any other pid (a reused pid naming the daemon or `loom stage complete`) is left alone and the lock is still recorded as finished. Test `cancel_skips_a_decoy_process` records a spawned `sleep 30` child's pid as the holder, runs the cancel path, asserts the child is still alive, then kills and reaps it (no process may outlive the test). `reconcile()` returns the SnapshotOutcome; a run failed when `outcome.state() != GraphState::Current` (serving a stale base means the build failed). A lock line that does not parse as four fields is treated as no lock, with no two-field compatibility parsing (loom/CLAUDE.md forbids migration routines).
        - M2 (holder protocol, design 9; the extra pass never runs after a `pid 0` write): the holder keeps its pid through every pass. After a pass, under the lock, it reads the line. If the line no longer names its own pid (a takeover), it writes nothing and exits. If `pending == 1`, it writes its own pid, a fresh epoch, `pending 0` and the pass's failure count, then runs the extra pass. Only when `pending == 0` does it write `pid 0` with the final failures: 0 after success, else the count carried forward plus one. Takeover of a dead or stale holder is a read-modify-write under the same lock that carries `failures` and `pending` forward. `reconcile()` failure accounting is internal: `loom hook reconcile-graph` still exits 0 whatever the outcome.
        - M2 tests in tests_wants_rebuild.rs, run by acceptance with --exact: `pending_pass_keeps_the_lease_held` (while the extra pass runs, the lock names the holder pid and a concurrent try_spawn is skipped and re-marks pending); `finish_after_takeover_keeps_the_new_holder` (a holder whose line another pid took over leaves that line byte-identical). `two_failed_runs_count_two_failures` fails each run through a real `ensure_snapshot` whose outcome is not Current (a project root that is not a git work tree), never a stubbed error. It asserts `failures == 2`, that `decide` refuses a spawn before `epoch + debounce_secs * 4`, and that it allows one after.
        - M2: tests_reconcile_graph.rs is 394 lines. New tests go in reconcile_graph/tests_wants_rebuild.rs (declared from reconcile_graph.rs), including `never_built_degraded_pack_spawns_nothing` (LOOM_WORK_DIR set, a never-built degraded pack, `SUPPRESSED_SPAWNS` unchanged after spawn_if_needed) and `two_failed_runs_count_two_failures` (through try_spawn and try_reconcile, ending with failures == 2). In tests_reconcile_graph.rs give `degraded_pack()` a semantic revision "abc", or the spawn tests at the refused/allowed cases pass vacuously or fail.
        - M2: context.rs anchors are `print_freshness_line` and `format_degraded`, not line numbers.
        - M2: refresh/tests_states.rs holds, under these exact names, `failed_build_serves_newest_older_base_as_stale` (a build failure with an older base yields revision = that base, serving = Some, state Stale) and `read_only_cache_reports_not_persisted` (a permission-denied write yields persisted == false and the describe() suffix). Acceptance runs both with --exact.
        - M3: `git check-attr --stdin -z linguist-vendored linguist-generated` is the one exception to run_git_checked (it cannot feed stdin). Spawn git with the runner's NO_HOOKS_ARGS, piped stdin and stdout, and write stdin from a separate thread while the caller reads stdout (verify/criteria/cache_ignore.rs feeds stdin but discards stdout; do not copy that). `git ls-files -z` uses `git::runner::run_git` (untrimmed bytes), not run_git_checked. M3's own many-path unit test uses the long paths of the census contract. Site ids `path@start-end` are byte offsets.
        - M3: enumerate with `git ls-files -s -z` and read each entry's mode: a symlink (120000) or gitlink (160000) is `unsupported`, never followed (the graph builder already skips them, refresh/source_graph/enumerate.rs). Take a file's size from `symlink_metadata`, read at most 1024 bytes for the generated markers, and apply the graph builder's Oversized cap before any in-memory extraction of another root. Test `census_never_follows_a_symlink`: a tracked `x.rs -> /dev/zero` is counted unsupported and the census returns.
        - M4: `footer_json` renders `ResolutionStats.by_provenance` beside retargeted, ambiguous and unresolved, so the field has a reader.
        - M4: `MapArgs` has no Default and a full literal in refresh/tests_snapshot.rs; replace it with `MapArgs::try_parse_from(["map", "--outline", "src.rs", "--json"])` so later flags (M4's, E3's --eval-edges) never break it. `--window` exits 2 for an unknown id and 3 for a changed file; the mapping lives in commands/map.rs. loom/tests/map_cli.rs spawns through helpers::loom_cmd() as the CONTRACT SURFACE says.
      files:
        - "src/context/resolve/neighbors.rs"
        - "src/context/resolve/impact.rs"
        - "src/context/resolve/tests_*.rs"
        - "src/context/resolve.rs"
        - "src/context/freshness.rs"
        - "src/context/refresh.rs"
        - "src/context/refresh/**"
        - "src/context/graph_store/fallback.rs"
        - "src/context/census/**"
        - "src/context/window.rs"
        - "src/context/window_tests.rs"
        - "src/context/mod.rs"
        - "src/context/retrieve/graph.rs"
        - "src/context/tests/store.rs"
        - "src/context/tests/retrieve_source.rs"
        - "src/orchestrator/signals/format/**"
        - "src/commands/knowledge/context.rs"
        - "src/commands/knowledge/bootstrap/tests.rs"
        - "src/commands/hook/reconcile_graph.rs"
        - "src/commands/hook/reconcile_graph/**"
        - "src/commands/hook/tests_reconcile_graph.rs"
        - "src/cli/types_ops.rs"
        - "src/cli/dispatch*.rs"
        - "src/commands/map.rs"
        - "src/commands/tests_map.rs"
        - "src/map/**"
        - "src/verify/goal_backward/reachable.rs"
        - "src/verify/impact_tests.rs"
        - "tests/map_cli.rs"
        - "tests/map_api_contracts.rs"
        - "../skills/loom-plan-writer/references/v2-contracts.md"
      before_stage:
        - command: "rg -q -F 'Print direct and transitive callers' src/commands/map.rs"
          exit_code: 0
          description: "BEFORE: --callers help promises transitive results the view does not produce"
        - command: "rg -q -F 'The walk follows resolved edges only' ../skills/loom-plan-writer/references/v2-contracts.md"
          exit_code: 0
          description: "BEFORE: the reachable doc says the walk follows resolved edges only"
      after_stage:
        - command: "cargo run --quiet -- map --help"
          exit_code: 0
          stdout_contains: ["one hop over call edges"]
          stdout_not_contains: ["transitive callers"]
          description: "AFTER: the binary's help no longer promises transitive callers"
        - command: "rg -q -F 'The walk follows resolved edges only' ../skills/loom-plan-writer/references/v2-contracts.md"
          exit_code: 1
          description: "AFTER: the reachable doc describes candidate-aware traversal"
        - command: "cargo test --test map_api_contracts callers_json_reports_call_site_lines -- --exact"
          exit_code: 0
          stdout_contains: ["1 passed"]
          description: "AFTER: callers report their call-site lines through the binary"
      acceptance:
        - "cargo build --all-targets"
        - "cargo clippy --all-targets -- -D warnings"
        - "cargo fmt --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps"
        - "cargo test --all-targets --no-fail-fast"
        - "cargo test --lib context::census::"
        - "cargo test --lib context::window"
        - "cargo test --lib context::refresh::tests_states::failed_build_serves_newest_older_base_as_stale -- --exact"
        - "cargo test --lib context::refresh::tests_states::read_only_cache_reports_not_persisted -- --exact"
        - "cargo test --lib commands::hook::reconcile_graph::tests_wants_rebuild::never_built_degraded_pack_spawns_nothing -- --exact"
        - "cargo test --lib commands::hook::reconcile_graph::tests_wants_rebuild::two_failed_runs_count_two_failures -- --exact"
        - "cargo test --lib commands::hook::reconcile_graph::tests_wants_rebuild::pending_is_never_lost_across_threads -- --exact"
        - "cargo test --lib commands::hook::reconcile_graph::tests_wants_rebuild::cancel_skips_a_decoy_process -- --exact"
        - "cargo test --lib commands::hook::reconcile_graph::tests_wants_rebuild::pending_pass_keeps_the_lease_held -- --exact"
        - "cargo test --lib commands::hook::reconcile_graph::tests_wants_rebuild::finish_after_takeover_keeps_the_new_holder -- --exact"
        - "../scripts/flake-check.sh commands::hook::reconcile_graph::"
        - "cargo test --lib census_never_follows_a_symlink"
        - "cargo test --test map_cli"
        - "cargo test --test graph_contract_contracts"
        - "cargo test --test binding_resolver_contracts"
        - "cargo test --test integration binary_spawn_guard"
        - "cargo test --test maintainability"
      artifacts:
        - "src/context/census/mod.rs"
        - "src/context/window.rs"
        - "src/commands/hook/reconcile_graph/tests_wants_rebuild.rs"
        - "tests/map_cli.rs"
      wiring:
        - source: "src/commands/hook/reconcile_graph.rs"
          pattern: "wants_rebuild\\(&pack"
          description: "spawn_if_needed gates on wants_rebuild"
        - source: "src/orchestrator/signals/format/brief.rs"
          pattern: "state\\(\\)\\.as_str\\(\\)"
          description: "the Knowledge Brief header prints the explicit graph state"
        - source: "src/commands/knowledge/context.rs"
          pattern: "state\\(\\)\\.as_str\\(\\)"
          description: "loom knowledge context prints the explicit graph state"
        - source: "src/context/retrieve/graph.rs"
          pattern: "source graph never built"
          description: "a never-built semantic layer is reported as degraded"
        - source: "src/commands/hook/reconcile_graph.rs"
          pattern: "mark_pending\\("
          description: "a request skipped while a holder runs marks the lease pending"
        - source: "src/cli/dispatch*.rs"
          pattern: "ReconcileGraph \\{ cancel"
          description: "the CLI hands --cancel to the reconcile hook"
        - source: "src/map/views/json.rs"
          pattern: "by_provenance"
          description: "map JSON renders resolution stats by provenance"
      reachable:
        - symbol: wants_rebuild
          from: spawn_if_needed
          min_confidence: 0.5
          description: "the prompt hook's spawn decision goes through wants_rebuild"
      wiring_tests:
        - name: "map help lists the new views"
          command: "cargo run --quiet -- map --help"
          success_criteria:
            exit_code: 0
            stdout_contains: ["--references", "--window", "--window-lines", "--census", "--root", "--lang", "--timings", "--evidence", "one hop over call edges"]
            stdout_not_contains: ["transitive callers"]
        - name: "reconcile-graph accepts --cancel"
          command: "cargo run --quiet -- hook reconcile-graph --help"
          success_criteria:
            exit_code: 0
            stdout_contains: ["--cancel"]
      contracts:
        - id: callers-json-reports-call-site-lines
          file: tests/map_api_contracts.rs
          test: callers_json_reports_call_site_lines
          scenario: "runs loom map --callers target --json in a TempDir git repo whose committed src/lib.rs is 'pub fn target() {}\\npub fn caller() {\\n    let _x = 1;\\n    target();\\n}\\n'"
          rejects: "a callers view that reports only the caller's declaration line (line_start 2) with no sites, instead of sites[0].line_start 4"
        - id: map-json-carries-schema-and-snapshot
          file: tests/map_api_contracts.rs
          test: map_json_carries_schema_and_snapshot
          scenario: "runs loom map --find-all target --json in the same kind of TempDir repo and reads git rev-parse HEAD"
          rejects: "JSON without schema loom-map/2 and a snapshot object whose state is current and whose base_revision equals HEAD"
        - id: impact-labels-substring-fallback
          file: tests/map_api_contracts.rs
          test: impact_labels_substring_fallback
          scenario: "runs loom map --impact targ --json where only target exists, then loom map --impact target --json"
          rejects: "an impact view that silently substitutes substring matches (no views.impact.match == substring for targ), or one that labels every match substring (target must give match == exact)"
        - id: map-timings-json-reports-phases-and-peak-rss
          file: tests/map_api_contracts.rs
          test: map_timings_json_reports_phases_and_peak_rss
          scenario: "runs loom map --find-all target --json --timings in the same kind of TempDir repo"
          rejects: "a --timings flag that adds an empty or partial object: JSON timings must hold numeric snapshot, load, resolve, query, render, total and peak_rss_kb, and stderr must contain the word total"
        - id: never-built-graph-does-not-request-rebuild
          file: tests/map_api_contracts.rs
          test: never_built_graph_does_not_request_rebuild
          scenario: "calls loom::commands::hook::reconcile_graph::wants_rebuild over the CONTRACT SURFACE truth table: never_built and unavailable with None and Some(\"degraded\") (all false), a stale Freshness with a non-empty revision (true), and a current Freshness with Some(\"degraded\") (true) and None (false)"
          rejects: "a hook gate that treats a never-built or unavailable graph like a stale one and requests a detached full rebuild"
        - id: census-reports-exclusions-beside-eligible
          file: tests/map_api_contracts.rs
          test: census_reports_exclusions_beside_eligible
          scenario: "commits src/a.rs 'pub fn a() {}\\n', vendor/v.rs 'pub fn v() {}\\n', src/gen.rs '// @generated\\npub fn g() {}\\n' and notes.xyz 'hello\\n', then runs loom map --census --json"
          rejects: "a census that counts vendored or generated files as eligible, or drops them from every total (expected eligible 1, vendored 1, generated 1, unsupported 1 files)"
        - id: census-handles-thousands-of-paths
          file: tests/map_api_contracts.rs
          test: census_handles_thousands_of_paths
          scenario: "commits 3000 one-line .rs files whose repo-relative paths are about 800 bytes each (13 nested directories of 60 characters, then f<n>.rs), then runs loom map --census --json; the paths total about 2.4 MB, above Linux ARG_MAX (2 MiB) and far above the 64 KiB pipe buffer for both check-attr's stdin and its output"
          rejects: "a census that builds one argv with every path (E2BIG) or writes all of check-attr's stdin before reading its stdout (a full-pipe deadlock); expected totals.eligible.files == 3000"
        - id: window-rejects-ids-outside-the-graph
          file: tests/map_api_contracts.rs
          test: window_rejects_ids_outside_the_graph
          scenario: "creates an outer TempDir holding repo/ (the git repo) and outside.txt containing SECRET-MARKER, then runs loom map --window '../outside.txt@0-5' with cwd repo/"
          rejects: "a window that joins the user-supplied path onto the project root and prints bytes of a file the graph does not contain, instead of exiting 2 with 'unknown id' on stderr"

    - id: resolved-view
      name: "Persisted resolved view"
      summary: "Each snapshot materializes a versioned resolved view, maintained by an incremental relink proven equal to a cold build, so warm loom map queries stop re-resolving the graph."
      stage_type: standard
      skills: ["loom-rust"]
      subagent_timeout_secs: 900
      working_dir: "loom"
      dependencies: ["binding-resolver", "map-api-freshness"]
      description: |
        Implement design section 12 of doc/plans/briefs/source-graph-mechanism/design.md.
        Use parallel subagents and skills to maximize performance.
        Spawn every worker BY AGENT TYPE with the fixed prompt plus "Your brief: <path>. Read it in full before anything else."
        Waves: V1; then V2. Workers NEVER spawn subagents.

        | Worker | Role | Tier | Files owned | Shared context | Brief path |
        | ------ | ---- | ---- | ----------- | -------------- | ---------- |
        | V1 | View core and relink | opus | src/context/view/mod.rs; src/context/view/identity.rs; src/context/view/build.rs; src/context/view/incremental.rs; src/context/view/deps.rs; src/context/view/tests.rs; src/context/view/tests_equivalence.rs; src/context/mod.rs; src/context/graph_store/layer_types.rs | design.md 7.0, 12 | doc/plans/briefs/source-graph-mechanism/resolved-view/v1-view-core.md |
        | V2 | Persistence and readers | sonnet | src/context/view/store.rs; src/context/view/tests_store.rs; src/context/graph_store/mod.rs; src/context/graph_store/prune.rs; src/context/graph_store/fallback.rs; src/context/refresh.rs; src/context/refresh/snapshot.rs; src/context/refresh/snapshot/describe.rs; src/context/refresh/source_graph.rs; src/context/refresh/source_graph/layer.rs; src/context/refresh/tests_view_lifecycle.rs; src/commands/map.rs; src/map/views/**; src/commands/knowledge/bootstrap/graph.rs; src/commands/clean/base_graphs.rs; src/context/config.rs; src/context/worktree_graph.rs; src/context/worktree_graph_tests.rs; tests/map_cli.rs | design.md 10, 12 | doc/plans/briefs/source-graph-mechanism/resolved-view/v2-view-integration.md |

        V1 owns view/mod.rs; after V1 finishes, V2 adds only its `mod store;` and `#[cfg(test)] mod tests_store;` lines there. V2's edits under src/map/views/ are the snapshot `resolver_version` and `view` keys, the `timings.view` key, and the json.rs key-set tests that pin them. V1's edit to graph_store/layer_types.rs is the ResolvedGraph derive line only. V2's edit to refresh.rs is the tests_view_lifecycle declaration only.

        CONTRACT SURFACE (loom/tests/resolved_view_contracts.rs):
        - loom::context::view::{ViewIdentity, ResolvedView, build_cold, relink, canonical_bytes, RESOLVER_VERSION}.
          build_cold(graph: ResolvedGraph, identity: ViewIdentity) -> ResolvedView.
          relink(previous: &ResolvedView, next: ResolvedGraph, identity: ViewIdentity) -> ResolvedView.
          canonical_bytes(&ResolvedView) -> anyhow::Result<Vec<u8>>.
          ViewIdentity { schema_version, base_revision, overlay_generation, extractor_digest, resolver_version } has pub fields; ViewIdentity::current(base_revision: &str, overlay_generation: &str).
        - The graphs passed to build_cold/relink hold extraction-time edges built as in stage binding-resolver's contract helper (extract_file + FileEntry::from_extraction); both functions resolve internally.
        - GraphStore::view(&self, revision: &str, overlay: Option<(&str, &str)>) -> anyhow::Result<ResolvedView> and GraphStore::view_path(&self, identity: &ViewIdentity, overlay: Option<(&str, &str)>) -> PathBuf. Store setup as in stage graph-contract's corrupt-base contract (TempDir git repo, ensure_snapshot(BaseOnly) materializes the base view).

        EXPECTED INTEGRITY EVENTS: TI-edit in the map/views/json.rs key-set tests where V2 adds the snapshot `resolver_version`/`view` keys and `timings.view`; file ONE dispute-integrity after the final review round, reason "design 12: the snapshot names the view it served".
        MEMORY: record mistakes, decisions and surprises via loom memory immediately (subagents too), including V2's cold/warm --timings numbers; never loom knowledge; never auto-memory.

        AMENDMENTS (pressure test; override the briefs and design.md; paste each worker's lines into its spawn prompt):
        - V1: `ResolvedGraph` derives Serialize and Deserialize (one derive line in graph_store/layer_types.rs; FileEntry already has serde). `ResolutionStats` and `EdgeRef` carry serde from binding-resolver. `ResolvedView` gains `#[serde(skip)] pub origin: ViewOrigin { Materialized, Built }`, which canonical_bytes never sees; it carries map's `"view": "materialized" | "built"`.
        - V1: `relink(previous, next, identity)` relinks only when `previous.identity` has the same schema_version, extractor_digest and resolver_version as `identity`; otherwise it returns `build_cold(next, identity)`. The relinked graph takes `base_revision` and `overlaid` from `next`, never from `previous`.
        - V1: no GraphIndex and no view/index.rs (non-goals: persisted name and adjacency indexes are deferred to the storage-engine decision). ResolvedView is { identity, graph, stats, deps, origin }.
        - V1: tests_equivalence.rs `identity_change_relinks_cold`, three cases, each where every file's content_hash is identical in previous and next: (1) previous extracted with different parser output (a previous entry whose edges or nodes differ, e.g. lexical-only) under a different extractor_digest; (2) a newly enabled pack: previous holds a `.java` file as a lexical gap entry under a digest without java, next holds it fully extracted; (3) the same graph with resolver_version differing and one previous binding altered. Each asserts canonical_bytes(relink(&prev, next, id)) == canonical_bytes(build_cold(next, id)) and that the relinked stats and bindings come from next's extraction.
        - V1: build_cold and relink each increment a `#[cfg(test)]` thread-local resolution-run counter in view/build.rs, read by `#[cfg(test)] pub(crate) fn resolution_runs() -> usize`. Thread-local, because lib tests run in parallel threads; no production cost.
        - V1: after removing the EdgeRefs of changed, removed and re-resolved edges from the DependencyIndex, drop every key whose list is empty; a cold build never has an empty key.
        - V1: extraction-time candidate sets (an unresolved Syntax edge with candidates at extraction) are never unbound or re-resolved (binding-resolver R2 amendment); `relink` excludes them from selection.
        - V1: tests_equivalence.rs holds, under these exact names, `seeded_edit_sequence_relink_equals_cold` (the 50-step seeded sequence), `same_file_ambiguity_survives_relink` (a same-file ambiguous call plus 9 namesakes across the graph; edit one namesake file), `namespace_change_reselects_import` (hand-built C# entries: a changed, not added, file starts declaring namespace A.B), and `relink_takes_header_from_next` (non-empty overlaid, a different base_revision). Acceptance runs each with --exact.
        - V2: `GraphStore::view` never persists a view when `load_base(revision)` is None (the empty graph `resolved()` returns for a missing base); it builds in memory and returns it. `publish_base` removes `graph/view/<revision>-*.json` for the revision it publishes. Test `missing_base_view_is_never_persisted`: view on a missing base, then publish, then view has nodes > 0.
        - V2: the worktree graph (worktree_graph.rs) stays read-only; V2 adds `pub(crate) fn load_view(&self, identity, overlay)` that never writes, and worktree_graph relinks from it in memory. The memory fallback gets a `view_fallback: RefCell<HashMap<PathBuf, ResolvedView>>` field (plus its line in `GraphStore::new`); `fall_back_to_memory`, `read_layer_or_memory` and `is_write_denied` widen to pub(crate) so view/store.rs can use them. Use the existing pub `base_dir()` and `overlay_dir(plan, stage)`; add no `graph_root()`/`overlay_root()` accessors.
        - V2: one `loom map` process deserializes the view at most once: `ensure_snapshot` stores the view it materialized in the GraphStore's in-process view cache, and `GraphStore::view` returns that entry when the identity matches. Record `loom map --find-all run_views --timings` cold and warm in a test repo with loom memory note; today's warm run in this repository takes about 1.66 s, and a warm run slower than that is recorded as a concern memory.
        - V2: the prune removes a base's views with the base and enforces the 2 GiB budget over graph/base plus graph/view, oldest unprotected revision first. The budget is a `RetrievalConfig` field (`graph_cache_budget_bytes`, default 2 GiB) read the way prune.rs reads `keep_base_graphs`, and it applies per main cache (`<main>/.loom/cache/context-v1`), shared by every worktree. Test `prune_enforces_byte_budget_oldest_first` drives `publish_base` with a 1 KiB budget and asserts the older base and its views are evicted, which also proves `prune_after_publish` calls the budget prune.
        - V2: `discard_overlay` removes the overlay's view.json with its graph.json (the lifecycle test asserts it). `ensure_snapshot` materializes the base view and, whenever it wrote or reused an overlay layer, the overlay view, so the prompt hook never builds and persists a view on its hot path. On materialize, delete sibling `graph/view/<revision>-*.json` files whose identity digest differs (a resolver_version bump would otherwise leave orphans until the revision is pruned). Concurrent builders of one view are allowed: the bytes are deterministic, `locked_write` takes a directory lock plus an atomic rename, and the last write wins. tests_view_lifecycle asserts `counters.bytes_read > 0` after a cold build.
        - V2: inside a stage sandbox the shared cache is read-only, so every in-stage `loom map` reports `persisted: false` and `view: "built"`. Warm-query numbers come only from TempDir or host runs; V2's report and post-merge operator check 2 are the sources. `loom clean` (commands/clean/base_graphs.rs) removes graph/view entries with their base and counts their bytes, and no longer returns early when graph/base is empty but graph/view is not.
        - V2: after this stage no file outside context/view names `resolve_graph`, even in a comment or `use` line: the after_stage check is `rg -w resolve_graph` over the three reader files. That grep proves spelling only; the warm-path proofs are the next two tests.
        - V2 tests in view/tests_store.rs, run by acceptance with --exact: `warm_view_load_runs_no_resolution` (TempDir git repo; ensure_snapshot(BaseOnly) through one GraphStore materializes the view; a second, fresh GraphStore over the same directories stands in for a second process, so its in-process cache is empty; `resolution_runs()` read before and after its `view(HEAD, None)` is unchanged, and origin is Materialized); `view_round_trips_through_disk_with_provenance_counts` (a repo with a cross-file call, so `stats.by_provenance` is non-empty; the view read back from disk through a fresh GraphStore has canonical_bytes equal to the materialized one and the same non-empty by_provenance; in-memory serialization alone does not count).
        - V2 binary test `warm_second_run_reads_the_materialized_view` in tests/map_cli.rs (helpers::loom_cmd(), TempDir git repo): two `loom map --find-all target --json` runs; the second's `snapshot.view` is "materialized".
        - V2: after switching worktree_graph.rs to relink from the base view, graph-contract's `stale_parser_base_entry_is_reextracted` passes unchanged (acceptance runs it with --exact): a base view built from stale-parser entries must not be copied into the worktree graph.
      files:
        - "src/context/view/**"
        - "src/context/mod.rs"
        - "src/context/graph_store/**"
        - "src/context/refresh.rs"
        - "src/context/refresh/**"
        - "src/commands/map.rs"
        - "src/map/views/**"
        - "tests/map_cli.rs"
        - "src/commands/knowledge/bootstrap/graph.rs"
        - "src/commands/clean/base_graphs.rs"
        - "src/context/config.rs"
        - "src/context/worktree_graph.rs"
        - "src/context/worktree_graph_tests.rs"
        - "tests/resolved_view_contracts.rs"
      before_stage:
        - command: "rg -q -F 'resolve_graph(&mut graph)' src/commands/map.rs"
          exit_code: 0
          description: "BEFORE: loom map resolves the whole graph in every process"
      after_stage:
        - command: "rg -q -w resolve_graph src/commands/map.rs src/commands/knowledge/bootstrap/graph.rs src/context/worktree_graph.rs"
          exit_code: 1
          description: "AFTER: no reader outside context/view resolves per process (design 12)"
        - command: "cargo test --test resolved_view_contracts relink_equals_cold_build_after_namesake_added -- --exact"
          exit_code: 0
          stdout_contains: ["1 passed"]
          description: "AFTER: incremental relink equals a cold build"
      acceptance:
        - "cargo build --all-targets"
        - "cargo clippy --all-targets -- -D warnings"
        - "cargo fmt --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps"
        - "cargo test --all-targets --no-fail-fast"
        - "cargo test --lib context::view::tests_equivalence::seeded_edit_sequence_relink_equals_cold -- --exact"
        - "cargo test --lib context::view::tests_equivalence::same_file_ambiguity_survives_relink -- --exact"
        - "cargo test --lib context::view::tests_equivalence::namespace_change_reselects_import -- --exact"
        - "cargo test --lib context::view::tests_equivalence::relink_takes_header_from_next -- --exact"
        - "cargo test --lib context::view::tests_store::missing_base_view_is_never_persisted -- --exact"
        - "cargo test --lib context::view::tests_store::prune_enforces_byte_budget_oldest_first -- --exact"
        - "cargo test --lib context::view::tests_equivalence::identity_change_relinks_cold -- --exact"
        - "cargo test --lib context::view::tests_store::warm_view_load_runs_no_resolution -- --exact"
        - "cargo test --lib context::view::tests_store::view_round_trips_through_disk_with_provenance_counts -- --exact"
        - "cargo test --lib context::worktree_graph::tests::stale_parser_base_entry_is_reextracted -- --exact"
        - "cargo test --test map_cli warm_second_run_reads_the_materialized_view -- --exact"
        - "cargo test --test map_cli"
        - "cargo test --test graph_contract_contracts"
        - "cargo test --test binding_resolver_contracts"
        - "cargo test --test map_api_contracts"
        - "cargo test --test integration knowledge_bootstrap"
        - "cargo test --test maintainability"
      artifacts:
        - "src/context/view/incremental.rs"
        - "src/context/view/store.rs"
      wiring:
        - source: "src/commands/map.rs"
          pattern: "graph_store\\.view\\("
          description: "loom map reads the materialized view"
        - source: "src/commands/knowledge/bootstrap/graph.rs"
          pattern: "graph_store\\.view\\(&snapshot\\.revision"
          description: "knowledge bootstrap reads the materialized view"
        - source: "src/context/worktree_graph.rs"
          pattern: "relink\\("
          description: "worktree graphs relink from the base view"
        - source: "src/context/refresh/snapshot.rs"
          pattern: "graph_store\\.view\\("
          description: "ensure_snapshot materializes the view"
      contracts:
        - id: relink-equals-cold-build-after-namesake-added
          file: tests/resolved_view_contracts.rs
          test: relink_equals_cold_build_after_namesake_added
          scenario: "G0 = {src/a.rs 'pub fn helper() {}\\n', src/b.rs 'pub fn run() {\\n    helper();\\n}\\n'}; G1 = G0 plus src/c.rs 'pub fn helper() {}\\n'; V0 = build_cold(G0, id0); compares canonical_bytes(relink(&V0, G1, id1)) with canonical_bytes(build_cold(G1, id1))"
          rejects: "a relink that re-resolves only edges originating in changed files, leaving src/b.rs's call bound UniqueName to src/a.rs#function:helper although helper is now ambiguous"
        - id: relink-equals-cold-build-after-target-removed
          file: tests/resolved_view_contracts.rs
          test: relink_equals_cold_build_after_target_removed
          scenario: "G0 = {src/a.rs 'pub fn helper() {}\\n', src/b.rs 'pub fn run() {\\n    helper();\\n}\\n'}; G1 = G0 without src/a.rs; V0 = build_cold(G0, id0); compares canonical_bytes(relink(&V0, G1, id1)) with canonical_bytes(build_cold(G1, id1))"
          rejects: "a relink that keeps src/b.rs's call bound to src/a.rs#function:helper, a node that no longer exists, because src/b.rs itself did not change"
        - id: corrupt-view-file-is-rebuilt-not-served
          file: tests/resolved_view_contracts.rs
          test: corrupt_view_file_is_rebuilt_not_served
          scenario: "in a TempDir git repo, runs ensure_snapshot(BaseOnly), overwrites GraphStore::view_path(&ViewIdentity::current(HEAD, \"\"), None) with the bytes 'not json', then calls GraphStore::view(HEAD, None)"
          rejects: "a loader that returns the parse error (or an empty view) instead of rebuilding a view whose identity.base_revision is HEAD, whose graph has nodes, and whose file parses again"

    - id: retrieval-delivery
      name: "Retrieval routing, explained neighbours and windows"
      summary: "Retrieval routes questions by intent, admits plainly asked symbol questions, explains why each graph neighbour appears, caps expansion by tokens, flags weak coverage or stale snapshots, suggests rg for literal text, and attaches up to two anchored source windows."
      stage_type: standard
      skills: ["loom-rust"]
      subagent_timeout_secs: 900
      working_dir: "loom"
      dependencies: ["map-api-freshness", "resolved-view"]
      description: |
        Implement design sections 11 and 13 of doc/plans/briefs/source-graph-mechanism/design.md.
        Use parallel subagents and skills to maximize performance.
        Spawn every worker BY AGENT TYPE with the fixed prompt plus "Your brief: <path>. Read it in full before anything else."
        Waves: D0; then D1, D2, D3 in ONE message. Territories are DISJOINT. Workers NEVER spawn subagents.
        D0 also fixes struct-literal sites crate-wide (listed in its brief) and creates the six test files wave 2 owns.

        | Worker | Role | Tier | Files owned | Shared context | Brief path |
        | ------ | ---- | ---- | ----------- | -------------- | ---------- |
        | D0 | Schema split and types | sonnet | src/context/schema.rs; src/context/schema/pack_types.rs; src/context/rank.rs; src/context/rank/candidate.rs; src/context/tests/mod.rs | design.md 13 | doc/plans/briefs/source-graph-mechanism/retrieval-delivery/d0-schema-foundation.md |
        | D1 | Intent routing | opus | src/context/rank_source.rs; src/context/rank_source/intent.rs; src/context/rank_source/routing.rs; src/context/rank_source/candidacy.rs; src/context/retrieve/request.rs; src/context/tests/rank_source_intent.rs; src/context/tests/rank_source_routing.rs | design.md 13 | doc/plans/briefs/source-graph-mechanism/retrieval-delivery/d1-intent-routing.md |
        | D2 | Expansion and caveats | sonnet | src/context/rank_source/expand.rs; src/context/pack.rs; src/context/pack/source_item.rs; src/context/tests/rank_source_explain.rs; src/context/tests/pack_caveats.rs | design.md 13 | doc/plans/briefs/source-graph-mechanism/retrieval-delivery/d2-expansion-caveats.md |
        | D3 | Surfaces and windows | sonnet | src/context/render.rs; src/orchestrator/signals/format/brief.rs; src/orchestrator/signals/format/brief_tests.rs; src/commands/knowledge/mod.rs; src/commands/knowledge/context.rs; src/commands/knowledge/context_render.rs; src/commands/hook/user_prompt.rs; src/commands/hook/user_prompt_compose.rs; src/commands/hook/worker_brief.rs; src/context/retrieve/graph.rs; src/context/retrieve/windows.rs; src/context/retrieve.rs; eval/retrieval-cases.yaml; src/context/tests/retrieve_windows.rs; src/context/tests/render_source_fields.rs | design.md 11, 13 | doc/plans/briefs/source-graph-mechanism/retrieval-delivery/d3-surfaces-windows.md |

        The table names each file's wave-2 owner. In wave 1, D0 also edits pack.rs, pack/source_item.rs, retrieve.rs, retrieve/request.rs, rank_source.rs and rank_source/expand.rs, creates the six test files wave 2 fills, and fixes the PackRequest, ContextItem, ContextPack, RankedCandidate and StageQuery literal sites and the Freshness::default() fixtures crate-wide (AMENDMENTS). D0 finishes all of them in wave 1. In wave 2, rank_source.rs and retrieve/request.rs are D1's alone (D1 writes `pub mod intent; mod routing;`, the routing call and the text-search step); expand.rs, pack.rs and pack/source_item.rs are D2's alone. D2 never edits rank_source.rs: D0 already added `pub const MAX_EXPANDED_TOKENS: usize = 300;` to expand.rs and `pub use expand::MAX_EXPANDED_TOKENS;` to rank_source.rs, and D2 applies the caveat factor in pack.rs only.

        CONTRACT SURFACE (loom/tests/retrieval_delivery_contracts.rs):
        - loom::context::rank_source::rank_source(query: &RankQuery, graph: &ResolvedGraph, config: &RetrievalConfig) -> Vec<RankedCandidate>, with RankQuery { text, required_ids, stage_dependency_ids, dependency_paths } (all pub; fill every field) and RetrievalConfig::default().
        - RankedCandidate { id, reasons: Vec<SelectionReason>, via: Option<NeighborVia>, .. } and SelectionReason::{SymbolQuestion, GraphNeighbor, ExactSymbol}.
          NeighborVia { seed: String, edge_kind: SourceEdgeKind, direction: EdgeDirection, provenance: EdgeProvenance, site_line: Option<usize> } and EdgeDirection::{Incoming, Outgoing}, public in loom::context::rank.
        - loom::context::rank_source::intent::{classify, QueryIntent}: classify(&str) -> QueryIntent, where QueryIntent::Literal { text: String } derives PartialEq and Debug.
        - loom::context::schema::TextSearchHint { pattern, command } with TextSearchHint::for_pattern(&str): command is "rg -n -F -- '<pattern>'" with each ' written as '\''.
        - Graphs are built as in stage binding-resolver's contract helper (extract_file + FileEntry::from_extraction + resolve_graph).
        - RankedCandidate.id is a ChunkId newtype with no PartialEq<str>; compare with `.id.as_str()`. RankQuery, RankedCandidate, NeighborVia and EdgeDirection are reachable as loom::context::rank::{...} (re-exported from the new context/rank/candidate.rs).

        NEGATIVE GUARDS: the contract prose-with-a-node-name-is-not-a-symbol-question freezes one; D1's 12 negative phrasings and the existing rank_source_candidacy tests stay acceptance tests and pass unchanged.
        EXPECTED INTEGRITY EVENTS: TI-edit in orchestrator/signals/format/brief_tests.rs source-section assertions, if raised; one dispute.
        CROSS-PLAN: rank.rs, the new context/rank/candidate.rs (under rank/**), context/tests/** and loom/eval/retrieval-cases.yaml are also owned by PLAN-web-host-graft-followthrough.md, whose pending worker R may itself factor a child module out of rank.rs; re-read them if that plan merged first.
        MEMORY: record mistakes, decisions and surprises via loom memory immediately (subagents too); never loom knowledge; never auto-memory.

        AMENDMENTS (pressure test; override the briefs and design.md; paste each worker's lines into its spawn prompt):
        - D0: context/rank.rs is 398 lines (design 15's split table). Move `RankQuery` and `RankedCandidate` (struct and impl, with their strength helpers) into a new context/rank/candidate.rs, add `NeighborVia` and `EdgeDirection` there, and `pub use` all four from rank.rs. rank.rs ends at or below 380 lines. `RankedCandidate` derives Debug, Clone, PartialEq, so NeighborVia derives the same.
        - D0: add `PackRequest.text_search: Option<TextSearchHint>`, fix every PackRequest literal (retrieve.rs, context/tests/pack_fixtures.rs, pack_source.rs, pack_twins.rs), and make `pack()` copy it into `ContextPack.text_search`. schema.rs re-exports `TextSearchHint` from schema/pack_types.rs, so loom::context::schema::TextSearchHint resolves.
        - D0: `Freshness::default()` now reads as NeverBuilt (empty revision), and caveats fire on any non-Current pack. Give the "healthy" fixtures `revision: "test-rev".into()`: context/tests/pack_source.rs, pack_fixtures.rs, pack_twins.rs, delivery.rs, schema.rs, orchestrator/signals/format/brief_tests.rs and commands/hook/worker_brief/test_support.rs (each where it builds semantic freshness with Freshness::default()).
        - D0: add `pub const MAX_EXPANDED_TOKENS: usize = 300;` to rank_source/expand.rs and `pub use expand::MAX_EXPANDED_TOKENS;` to rank_source.rs. Do not declare `pub mod intent;`: D1 creates intent.rs and declares it. Confidence::from_reasons uses `matches!` with a Low fallback; only the Display match is exhaustive.
        - D1: set `PackRequest.text_search` in `build_pack_request` when `classify(&query.text)` is Literal; nothing sets it on the pack directly. Write the negative-phrasing test as `negative_phrasings_admit_nothing` (12 phrasings) and the relationship routing test as `relationship_query_seeds_direct_neighbours`.
        - D2: the caveat score factor lives in pack.rs; D2 does not edit rank_source.rs.
        - D3: surfaces. The stage signal is `Surface::StageBrief`; `loom knowledge context` builds StageQuery by literal, so set `surface: Surface::Cli` there; the prompt hook (commands/hook/user_prompt.rs) and the subagent worker brief (commands/hook/worker_brief.rs) are `Surface::Hook` and attach no windows. commands/knowledge/mod.rs declares context_render. Name the hook-floor test `symbol_question_item_passes_the_emit_floor`. Anchor user_prompt_compose.rs edits on `admits` and `clears_item_floor`, not line numbers.
        - D3: a new eval case that must not surface a source item uses `forbid` on the source ids; `abstain` applies to the whole hook and cannot express "no source item".
      files:
        - "src/context/schema.rs"
        - "src/context/schema/**"
        - "src/context/pack.rs"
        - "src/context/pack/**"
        - "src/context/retrieve.rs"
        - "src/context/retrieve/**"
        - "src/context/rank.rs"
        - "src/context/rank/candidate.rs"
        - "src/context/rank_source.rs"
        - "src/context/rank_source/**"
        - "src/context/render.rs"
        - "src/context/tests/**"
        - "src/context/fuse.rs"
        - "src/orchestrator/signals/**"
        - "src/commands/knowledge/**"
        - "src/commands/hook/**"
        - "eval/retrieval-cases.yaml"
        - "tests/retrieval_delivery_contracts.rs"
      before_stage:
        - command: "rg -q -F 'SymbolQuestion' src/context/schema.rs"
          exit_code: 1
          description: "BEFORE: no symbol-question selection reason exists"
      after_stage:
        - command: "cargo test --test retrieval_delivery_contracts symbol_question_admits_a_one_word_name -- --exact"
          exit_code: 0
          stdout_contains: ["1 passed"]
          description: "AFTER: a plainly asked symbol question admits the named node"
      acceptance:
        - "cargo build --all-targets"
        - "cargo clippy --all-targets -- -D warnings"
        - "cargo fmt --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps"
        - "cargo test --all-targets --no-fail-fast"
        - "cargo test --lib context::tests::rank_source"
        - "cargo test --lib negative_phrasings_admit_nothing"
        - "cargo test --lib relationship_query_seeds_direct_neighbours"
        - "cargo test --lib symbol_question_item_passes_the_emit_floor"
        - "cargo test --test integration context_catalog"
        - "cargo test --test graph_contract_contracts"
        - "cargo test --test binding_resolver_contracts"
        - "cargo test --test map_api_contracts"
        - "cargo test --test resolved_view_contracts"
        - "cargo test --test maintainability"
      artifacts:
        - "src/context/rank_source/intent.rs"
        - "src/context/rank_source/routing.rs"
        - "src/context/retrieve/windows.rs"
        - "src/context/rank/candidate.rs"
      wiring:
        - source: "src/context/rank_source.rs"
          pattern: "routing::"
          description: "rank_source routes by query intent"
        - source: "src/context/retrieve.rs"
          pattern: "attach_windows\\("
          description: "retrieval attaches anchored windows after selection"
        - source: "src/context/retrieve/graph.rs"
          pattern: "graph_store\\.view\\("
          description: "retrieval reads the resolved view"
        - source: "src/commands/hook/user_prompt_compose.rs"
          pattern: "SymbolQuestion"
          description: "the prompt hook floor admits symbol questions"
        - source: "src/context/retrieve/request.rs"
          pattern: "for_pattern\\("
          description: "a literal query puts a text-search hint on the pack request"
        - source: "src/orchestrator/signals/format/brief.rs"
          pattern: "Literal text:"
          description: "the stage brief renders the text-search hint"
        - source: "src/commands/knowledge/context_render.rs"
          pattern: "Literal text:"
          description: "loom knowledge context renders the text-search hint"
        - source: "src/commands/hook/user_prompt.rs"
          pattern: "Surface::Hook"
          description: "the prompt hook asks as the Hook surface, so it attaches no windows"
        - source: "src/commands/hook/worker_brief.rs"
          pattern: "Surface::Hook"
          description: "subagent worker briefs attach no windows"
        - source: "src/commands/knowledge/context.rs"
          pattern: "Surface::Cli"
          description: "loom knowledge context asks as the CLI surface"
      reachable:
        - symbol: attach_windows
          from: retrieve_for_stage
          min_confidence: 0.5
          description: "stage retrieval attaches windows after selection"
      contracts:
        - id: symbol-question-admits-a-one-word-name
          file: tests/retrieval_delivery_contracts.rs
          test: symbol_question_admits_a_one_word_name
          scenario: "builds a graph from src/lexer.rs 'pub fn tokenize() {}\\n' and calls rank_source with RankQuery text 'what does tokenize do'"
          rejects: "candidacy that still refuses the unmarked one-word name (no candidate src/lexer.rs#function:tokenize with reason SymbolQuestion)"
        - id: literal-query-is-classified-with-its-text
          file: tests/retrieval_delivery_contracts.rs
          test: literal_query_is_classified_with_its_text
          scenario: "calls classify('where do we log \"connection refused\"')"
          rejects: "splitting the quoted phrase into symbol terms (QueryIntent::General) instead of QueryIntent::Literal { text: \"connection refused\" }"
        - id: literal-hint-escapes-single-quotes
          file: tests/retrieval_delivery_contracts.rs
          test: literal_hint_escapes_single_quotes
          scenario: "calls TextSearchHint::for_pattern(\"it's broken\")"
          rejects: "a hint that pastes the pattern inside single quotes unescaped; the command must equal rg -n -F -- 'it'\\''s broken'"
        - id: graph-neighbors-carry-their-edge
          file: tests/retrieval_delivery_contracts.rs
          test: graph_neighbors_carry_their_edge
          scenario: "builds a graph from src/lib.rs 'pub fn seed() {}\\npub fn user() {\\n    seed();\\n}\\n' and calls rank_source with RankQuery text '`seed`'"
          rejects: "expansion that drops the introducing edge: the GraphNeighbor candidate src/lib.rs#function:user must carry via with seed src/lib.rs#function:seed, edge_kind Calls, direction Outgoing and site_line Some(3)"
        - id: relationship-query-seeds-direct-callers
          file: tests/retrieval_delivery_contracts.rs
          test: relationship_query_seeds_direct_callers
          scenario: "builds a graph from src/lib.rs 'pub fn target() {}\\npub fn caller() {\\n    target();\\n}\\npub fn bystander() {}\\n' and calls rank_source with RankQuery text 'who calls target'"
          rejects: "routing that ignores the relationship intent: a GraphNeighbor candidate src/lib.rs#function:caller with via.seed src/lib.rs#function:target must be present, and src/lib.rs#function:bystander must not be"
        - id: prose-with-a-node-name-is-not-a-symbol-question
          file: tests/retrieval_delivery_contracts.rs
          test: prose_with_a_node_name_is_not_a_symbol_question
          scenario: "builds a graph from src/lexer.rs 'pub fn tokenize() {}\\n', calls rank_source with RankQuery text 'please tokenize the input before parsing', and calls classify('what does the plan say')"
          rejects: "a symbol-question rule loose enough to admit prose: no candidate may carry SelectionReason::SymbolQuestion, and classify must return QueryIntent::General"

    - id: edge-quality-eval
      name: "Edge-quality evaluation"
      summary: "Adds an offline evaluator that scores the graph against hand-labelled corpora for all twelve dialects, with thresholds published before any holdout, a loom map --eval-edges command, and the operator protocol for cross-project agent comparisons."
      stage_type: standard
      skills: ["loom-rust"]
      subagent_timeout_secs: 900
      working_dir: "loom"
      dependencies: ["language-packs", "binding-resolver", "resolved-view"]
      description: |
        Implement design section 14 of doc/plans/briefs/source-graph-mechanism/design.md.
        Use parallel subagents and skills to maximize performance.
        Spawn every worker BY AGENT TYPE with the fixed prompt plus "Your brief: <path>. Read it in full before anything else."
        Waves: E3; then E1 and E2 in ONE message; then, only if E1 or E2 report loom defects, ONE loom-senior-software-engineer (opus). That fixer mends each defect at its source in loom/src/context/extract/** or loom/src/context/resolve/**, from a fresh brief the main agent writes quoting the reports and design sections 6-7.
        Relabelling a corpus to match loom output is forbidden. Workers NEVER spawn subagents.

        | Worker | Role | Tier | Files owned | Shared context | Brief path |
        | ------ | ---- | ---- | ----------- | -------------- | ---------- |
        | E3 | Evaluator, CLI, protocol | sonnet | src/context/eval_edges/mod.rs; src/context/eval_edges/labels.rs; src/context/eval_edges/metrics.rs; src/context/eval_edges/build.rs; src/context/eval_edges/tests.rs; src/context/mod.rs; eval/edge-quality-thresholds.yaml; src/commands/map.rs; ../doc/source-graph-evaluation.md | design.md 10, 14 | doc/plans/briefs/source-graph-mechanism/edge-quality-eval/e3-evaluator.md |
        | E1 | Core corpora | sonnet | tests/fixtures/source/labeled/rust; tests/fixtures/source/labeled/typescript; tests/fixtures/source/labeled/tsx; tests/fixtures/source/labeled/javascript; tests/fixtures/source/labeled/python; tests/fixtures/source/labeled/go | design.md 14.1 | doc/plans/briefs/source-graph-mechanism/edge-quality-eval/e1-corpora-core.md |
        | E2 | Pack corpora | sonnet | tests/fixtures/source/labeled/java; tests/fixtures/source/labeled/csharp; tests/fixtures/source/labeled/ruby; tests/fixtures/source/labeled/php; tests/fixtures/source/labeled/c; tests/fixtures/source/labeled/cpp | design.md 14.1 | doc/plans/briefs/source-graph-mechanism/edge-quality-eval/e2-corpora-packs.md |

        E3 also owns the --eval-edges rendering it adds under loom/src/map/views/.

        CONTRACT SURFACE (loom/tests/edge_quality_contracts.rs):
        - loom::context::eval_edges::{evaluate_dir, load_thresholds, EdgeQualityReport, Thresholds}.
          evaluate_dir(&Path) -> anyhow::Result<EdgeQualityReport>.
          EdgeQualityReport { dialect, declaration_recall: f64, target_precision: f64, target_recall: f64, unresolved_rate: f64, ambiguous_rate: f64, false_high_confidence: usize, impact_false_negatives: BTreeMap<usize, usize>, failures: Vec<String> }.
          load_thresholds(&Path) -> anyhow::Result<Thresholds>; Thresholds::check(&self, &EdgeQualityReport) -> Vec<String> (empty when every threshold holds).
        - The corpus format is design 14.1 (labels.yaml with dialect, declarations, references with expect {target | external | ambiguous}, impact, optional syntax_error_files).
        - Corpora live under tests/fixtures/source/labeled/<dialect-id> and thresholds at eval/edge-quality-thresholds.yaml (both relative to the loom package root).
        - Dialects to cover: every loom::context::extract::dialect::DIALECTS row whose GrammarPack::compiled() is true.
        - Temp corpora in contracts are written to a TempDir. A Rust corpus there is src/lib.rs plus labels.yaml.
        - A binary test spawns loom only through helpers::loom_cmd(), as stage map-api-freshness's CONTRACT SURFACE says.

        EXPECTED INTEGRITY EVENTS: TI-edit in commands/tests_map.rs `map_without_a_view_flag_names_all_available_views` (the require_view message gains --eval-edges); one dispute.
        MEMORY: record mistakes, decisions and surprises via loom memory immediately (subagents too), including every loom defect a corpus exposed; never loom knowledge; never auto-memory.

        AMENDMENTS (pressure test; override the briefs and design.md; paste each worker's lines into its spawn prompt):
        - E3: the evaluator builds its graph with `crate::context::view::build_cold` (design 12: nothing outside context/view calls resolve_graph).
        - E3: a reference label matches an edge from its path whose site covers the labelled line and whose `symbol` equals the label's symbol or ends with it after a `::` or `.` separator (edge symbols keep the written qualifier: `util::parse`, `Widget::new`).
        - E3: a ratio whose denominator is 0 is a failure, never a pass. `Thresholds::check` also fails a corpus that lacks at least one target label, one external label, one ambiguous label, one impact label and one syntax_error_files entry. `--eval-edges` exits 2 when the directory holds no labelled corpus, 1 when any threshold fails, 0 otherwise, and always prints the summary line `<n> corpora, <f> failing`. `--json` and `--thresholds <FILE>` (default loom/eval/edge-quality-thresholds.yaml) are the only other flags.
        - E3: doc/source-graph-evaluation.md carries the headings `## Census`, `## Holdout labelling`, `## Agent-task comparison`, `## Metrics and stratification` and `## Release rule`, with design 14.3's content under them.
        - E3: MapArgs gains --eval-edges through clap only; commands/tests_map.rs's pinned require_view message is updated (the TI event above).
        - E2 (ruby corpus): include app/x.rb and lib/x.rb, each 'def helper\nend\n'; app/a.rb "require_relative 'x'\ndef run_a\n  helper()\nend\n"; app/b.rb "require 'x'\ndef run_b\n  helper()\nend\n". Label the helper() reference in app/a.rb with target app/x.rb#function:helper and the one in app/b.rb with target lib/x.rb#function:helper (Ruby semantics: require_relative is file-relative; require searches the load path, lib/ in a gem layout). These labels are fixed by the plan: a failure here is a loom defect for the wave-3 fixer, never a relabel.
        - Fixer (wave 3): a fix to a walk bumps that dialect's `extractor_version`; a fix to resolution rules bumps `RESOLVER_VERSION` (context/view). Without the bumps, cached layers and views keep serving the old output.
        - Known gap, recorded for knowledge-distill: EXCLUDED_ROOTS matches only a first path segment, so the corpora under loom/tests/fixtures/source/labeled/ enter this repository's own graph, and their common names (helper, parse, save) turn some of loom's own unique-name binds into candidate sets.
      files:
        - "src/context/eval_edges/**"
        - "src/context/mod.rs"
        - "src/context/extract/**"
        - "src/context/resolve.rs"
        - "src/context/resolve/**"
        - "src/context/view/**"
        - "src/commands/map.rs"
        - "src/commands/tests_map.rs"
        - "src/map/**"
        - "eval/edge-quality-thresholds.yaml"
        - "tests/fixtures/source/labeled/**"
        - "tests/edge_quality_contracts.rs"
        - "../doc/source-graph-evaluation.md"
      before_stage:
        - command: "rg -q -F 'eval-edges' src/commands/map.rs"
          exit_code: 1
          description: "BEFORE: loom map has no edge-quality evaluator"
      after_stage:
        - command: "cargo run --quiet -- map --eval-edges tests/fixtures/source/labeled"
          exit_code: 0
          stdout_contains: ["12 corpora, 0 failing"]
          description: "AFTER: all twelve labelled corpora meet the published thresholds"
        - command: "rg -q -F '## Release rule' ../doc/source-graph-evaluation.md"
          exit_code: 0
          description: "AFTER: the operator protocol states the release rule"
      acceptance:
        - "cargo build --all-targets"
        - "cargo clippy --all-targets -- -D warnings"
        - "cargo fmt --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps"
        - "cargo test --all-targets --no-fail-fast"
        - "cargo test --lib context::eval_edges::"
        - "cargo clippy --no-default-features --lib --bins -- -D warnings"
        - "cargo test --no-default-features --features source-graph --lib context::extract::"
        - "cargo run --quiet -- map --eval-edges tests/fixtures/source/labeled"
        - "cargo test --test graph_contract_contracts"
        - "cargo test --test language_packs_contracts"
        - "cargo test --test binding_resolver_contracts"
        - "cargo test --test resolved_view_contracts"
        - "cargo test --test integration source_graph_fixtures"
        - "cargo test --test maintainability"
      artifacts:
        - "src/context/eval_edges/metrics.rs"
        - "eval/edge-quality-thresholds.yaml"
        - "tests/fixtures/source/labeled/rust/labels.yaml"
        - "tests/fixtures/source/labeled/cpp/labels.yaml"
      wiring:
        - source: "src/commands/map.rs"
          pattern: "eval_edges::"
          description: "loom map --eval-edges runs the evaluator"
      contracts:
        - id: false-high-confidence-is-counted
          file: tests/edge_quality_contracts.rs
          test: false_high_confidence_is_counted
          scenario: "writes a TempDir corpus with src/lib.rs 'fn helper() {}\\nfn other() {}\\nfn run() {\\n    helper();\\n}\\n' and a labels.yaml whose reference at line 4, symbol helper, expects target src/lib.rs#function:other, then calls evaluate_dir"
          rejects: "an evaluator that counts only unresolved edges as misses; the local-name edge to helper at 0.8 against a label expecting other must give false_high_confidence == 1"
        - id: impact-false-negatives-are-counted-by-depth
          file: tests/edge_quality_contracts.rs
          test: impact_false_negatives_are_counted_by_depth
          scenario: "writes a TempDir corpus with src/lib.rs 'fn helper() {}\\nfn run() {\\n    helper();\\n}\\nfn lonely() {}\\n' and an impact label start src/lib.rs#function:helper, depth 1, expect [src/lib.rs#function:run, src/lib.rs#function:lonely], then calls evaluate_dir"
          rejects: "an evaluator that ignores impact labels or counts only the ids it found; impact_false_negatives must map depth 1 to 1"
        - id: every-dialect-meets-published-thresholds
          file: tests/edge_quality_contracts.rs
          test: every_dialect_meets_published_thresholds
          scenario: "asserts load_thresholds(eval/edge-quality-thresholds.yaml) equals design 14.2 exactly (declaration_recall 1.0, false_high_confidence 0, target_precision 0.95, target_recall 0.5, impact_false_negatives_depth_1 0); then, for every DIALECTS row whose pack is compiled, evaluates tests/fixtures/source/labeled/<id> and checks it against those thresholds"
          rejects: "a threshold gate that skips dialects without a corpus (a missing corpus directory must fail), lets any compiled dialect ship with a failing threshold, or passes because the thresholds file was loosened"
        - id: thin-corpus-fails-thresholds
          file: tests/edge_quality_contracts.rs
          test: thin_corpus_fails_thresholds
          scenario: "writes a TempDir corpus with src/lib.rs 'fn a() {}\\n' and a labels.yaml holding `dialect: rust` and empty declarations, references and impact lists, calls evaluate_dir, and checks the report against load_thresholds(eval/edge-quality-thresholds.yaml)"
          rejects: "an evaluator whose 0/0 ratios pass, or a check that accepts a corpus with no target, external, ambiguous or impact label and no syntax-error file; check() must return a non-empty list"

    - id: integration-verify
      name: "Integration verification"
      summary: "Confirms the evidence model, new languages, resolver, map API, resolved view, retrieval changes and evaluator are wired into the running program, and that the full suite, lints and degraded builds pass."
      stage_type: integration-verify
      skills: ["loom-rust", "loom-security-audit"]
      working_dir: "."
      dependencies: ["retrieval-delivery", "edge-quality-eval"]
      description: |
        Final verification of PLAN-source-graph-mechanism. Verify FUNCTIONAL INTEGRATION, not just tests passing. NEVER Claude Code auto-memory.
        Use parallel subagents and skills to maximize performance.
        CONTEXT: read this plan, doc/plans/briefs/source-graph-mechanism/design.md, loom memory show --all, and the knowledge sections the brief quotes.
        BUILD AND TEST (zero tolerance; fix every warning and failure through an engineer subagent, sonnet or opus by the rubric): the full suite, clippy with warnings denied, fmt, and the degraded builds in acceptance.
        CODE REVIEW: spawn parallel loom-code-reviewer subagents, each recording a loom-review block:
        - security: loom map --window path handling, reconcile --cancel signalling, census git invocations, TextSearchHint escaping;
        - architecture: evidence-class honesty (only Structural at 1.0; no bind across families; dynamic receivers never bound), view identity and relink equivalence, dialect precedence;
        - test coverage: contracts, labelled corpora, and the negative retrieval guards. The reviewer spot-checks the stages' "mutation: <id> red" memories by re-applying two `rejects:` implementations of its choice and confirming the contract fails.
        Fix or dispute every finding; never defer one.
        SUGGESTIONS: weigh every pending reviewer suggestion the signal lists; resolve each implemented one with loom memory resolve <id> --outcome implemented --reason <what changed>.
        FUNCTIONAL: every stage's reachable and wiring checks re-run on the merged tree. The binary tests (loom/tests/map_cli.rs, map_api_contracts.rs) drive loom map against temp repos, and the acceptance runs --eval-edges over every corpus.
        Run every acceptance command once in-session before loom stage complete, so the degraded-feature builds are warm (each acceptance command has a 300 s cap).
        Do not put loom map against this repository, loom knowledge eval, or scripts/retrieval-ab in acceptance: they open the shared cache. Try them as an in-session smoke if the sandbox allows, and record the outcome in loom memory either way.
        Record discoveries to loom memory for knowledge-distill, including every knowledge file the tree now contradicts: loom memory note "stale-knowledge: <file>#<heading> claims X; the tree does Y".
      acceptance:
        - "cargo test --manifest-path loom/Cargo.toml --all-targets --no-fail-fast"
        - "cargo clippy --manifest-path loom/Cargo.toml --all-targets -- -D warnings"
        - "cargo fmt --manifest-path loom/Cargo.toml --all -- --check"
        - "RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path loom/Cargo.toml --workspace --all-features --no-deps"
        - "cd loom && cargo audit --no-fetch"
        - "scripts/flake-check.sh quota::"
        - "scripts/flake-check.sh process::"
        - "scripts/flake-check.sh verdict_apply_tests::"
        - "scripts/flake-check.sh stalled_judge_tests::"
        - "cargo clippy --manifest-path loom/Cargo.toml --no-default-features --lib --bins -- -D warnings"
        - "cargo clippy --manifest-path loom/Cargo.toml --no-default-features --features source-graph --lib --bins -- -D warnings"
        - "cargo clippy --manifest-path loom/Cargo.toml --no-default-features --features source-graph-wave-b --lib --bins -- -D warnings"
        - "cargo clippy --manifest-path loom/Cargo.toml --no-default-features --features source-graph-wave-c --lib --bins -- -D warnings"
        - "cargo test --manifest-path loom/Cargo.toml --no-default-features --features source-graph --lib context::extract::"
        - "cargo run --manifest-path loom/Cargo.toml --quiet -- map --eval-edges loom/tests/fixtures/source/labeled"
        - "rg -q -F '## Release rule' doc/source-graph-evaluation.md"
        - "rg -q -F 'above 0.2' skills/loom-plan-writer/references/v2-contracts.md"
      wiring_tests:
        - name: "map help lists every new view"
          command: "cargo run --manifest-path loom/Cargo.toml --quiet -- map --help"
          success_criteria:
            exit_code: 0
            stdout_contains: ["--references", "--window", "--window-lines", "--census", "--root", "--lang", "--timings", "--evidence", "--eval-edges", "one hop over call edges"]
            stdout_not_contains: ["transitive callers"]
      files:
        - "loom/**"
        - "doc/**"
        - "skills/loom-plan-writer/references/v2-contracts.md"

    - id: knowledge-distill
      name: "Knowledge distillation"
      summary: "Records what this plan changed and learned in the knowledge base (evidence classes, dialects and packs, resolution rules, map API, resolved view, retrieval routing, evaluation) and updates the README's loom map section."
      stage_type: knowledge-distill
      working_dir: "."
      dependencies: ["integration-verify"]
      description: |
        Curate all stage memories into permanent knowledge, and update user docs. NEVER Claude Code auto-memory.
        SINGLE-AGENT: do NOT spawn subagents. Memories are compact summaries: lean on them and keep code spot-reads narrow.
        START with loom memory pending --group (corrections, mistakes, decisions, other, suggestions). Read this plan and the knowledge sections it touches.
        CORRECTIONS FIRST: apply every stale-knowledge: memory in place with loom knowledge replace-section <file> "<heading>" "<body>", never with loom knowledge update. Read each command's output: a non-matching heading appends and says so.
        CURRENT TRUTH: rewrite these to the new tree:
        - architecture/source-graph.md: the honesty contract with the seven evidence classes and their constants, sites and candidate sets, node identity with @sig8 and symbol_key, the dialect table and grammar packs, the schema version and corrupt-layer recovery, freshness states, the reconcile lease, and the resolved view with relink equivalence;
        - architecture/context-retrieval*.md: intent routing, explained neighbours, token cap, caveats, windows, and the view-backed graph;
        - entry-points/context-and-source-graph.md: the new loom map views and flags, context::census, context::window, context::view, context::eval_edges;
        - stack.md: the grammar packs, pins and features, replacing the "one feature, not six" rule with the per-pack capability-gap rule;
        - concerns: known gaps (C/C++ macros and templates, reflection and dynamic dispatch, JS #private members, Ruby bare calls, Kotlin/Swift/wave D not supported, the storage-engine decision awaiting --timings data, the agent-task comparison not yet run, the labelled corpora entering loom's own graph because EXCLUDED_ROOTS matches only a first path segment).
        Tier-route by size; INDEX.md regenerates on every write.
        MISTAKES: every mistake memory becomes a prevention rule in the right mistakes topic.
        README: update the loom map section (README.md, the passage listing --callers/--callees/--json) to the new views and flags. Skip CONTRIBUTING unless a memory says it changed.
        SUGGESTIONS: record every unimplemented reviewer suggestion in concerns or its topic, then resolve it promoted, merged or discarded.
        RECEIPTS: every entry taken into knowledge gets loom memory resolve <id> --outcome promoted|merged|discarded|deferred right after the write that used it. Finish with loom memory pending --strict.
        LAST, if this stage removed structural issues: loom knowledge check --write-baseline doc/loom/knowledge/check-baseline.txt
      acceptance:
        - "loom knowledge check --strict --baseline doc/loom/knowledge/check-baseline.txt"
        - "loom memory pending --strict"
        - "rg -q -F -- '--census' README.md"
        - "rg -q -F -- '--references' README.md"
        - "rg -q -F -- '--eval-edges' README.md"
      files:
        - "doc/loom/knowledge/**"
        - "README.md"
        - "CONTRIBUTING.md"
```

<!-- END loom METADATA -->
