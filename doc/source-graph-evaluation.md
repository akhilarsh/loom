# Source graph evaluation

The procedure that decides whether a dialect or resolver capability reaches agent guidance.
The fixture half is automated: `loom map --eval-edges` and the cargo test
`every_dialect_meets_published_thresholds`. The holdout and agent-comparison halves are run
by an operator by hand.

## Census

The census reports what the graph can and cannot see in a checkout: per-language file counts,
coverage tiers, parse failures and unresolved-edge rates. Run it across every consenting
project before labelling anything, so the holdout set is chosen from the observed spread.

```bash
loom map --census --root ~/src/project-a --root ~/src/project-b --root ~/src/project-c
loom map --census --json --root ~/src/project-a > census-project-a.json
```

- `--root` is repeatable and names a git work tree. The current checkout is always included.
- Record the census output next to the evaluation results. Every later stratum (language,
  coverage tier) is read from it.
- Select holdout projects so that every dialect under evaluation has at least one project
  and no project is one the fixtures were derived from.

## Holdout labelling

A holdout project is labelled before loom runs against it, so the labels state what the
language binds.

1. Copy the corpus format from any fixture under `loom/tests/fixtures/source/labeled/<dialect>/`.
   A corpus is a directory holding the project's source files and a `labels.yaml`.
2. Copy only the files the labels need. Keep their relative paths. Store a file that does not
   parse with a `.txt` suffix and list it, without the suffix, under `syntax_error_files`.
3. Write `labels.yaml` by reading the code:

```yaml
dialect: rust
declarations:            # every declaration a compiler would see
  - {path: src/lib.rs, kind: function, scope: [Widget, new], line: 4}
references:              # every call or reference site worth labelling
  - {path: src/main.rs, line: 7, symbol: parse, expect: {target: "src/util.rs#function:parse"}}
  - {path: src/main.rs, line: 9, symbol: fetch, expect: {external: true}}
  - {path: src/main.rs, line: 11, symbol: save, expect: {ambiguous: ["src/a.rs#function:save", "src/b.rs#function:save"]}}
impact:
  - {start: "src/util.rs#function:parse", depth: 2, expect: ["src/main.rs#function:main"]}
syntax_error_files: [src/broken.rs]
```

- `expect` holds exactly one of `target`, `external` or `ambiguous`.
- A `symbol` matches an edge written `symbol`, `qualifier::symbol` or `receiver.symbol`.
- Cover duplicate same-file names, overloads where the language has them, aliases, re-exports,
  receiver calls, a dynamic receiver, an external import, nested scopes, a syntax-error file,
  and a test file beside a production file.
- Commit the labels before the first evaluation run. A label is never edited to match output.
  A label that exposes an extractor or resolver defect is fixed at the source, with the
  extractor version bumped for a walk fix and `RESOLVER_VERSION` bumped for a resolution fix.

Then evaluate. The directory may be one corpus or a directory of corpora:

```bash
loom map --eval-edges ~/holdout/project-a
loom map --eval-edges ~/holdout --json
loom map --eval-edges ~/holdout --thresholds loom/eval/edge-quality-thresholds.yaml
```

The command prints one line per dialect with every metric, then the misses (at most 20 per
dialect), then `<n> corpora, <f> failing`. It exits 2 when the directory holds no labelled
corpus, 1 when any threshold fails, and 0 otherwise. It is pure and in memory: it opens no
`.loom/` state and runs in any directory.

## Agent-task comparison

The comparison measures whether the graph helps an agent finish real tasks. It has four modes:

| Mode | The agent may use |
| --- | --- |
| `rg-first` | `rg`, `fd` and file reads only |
| `graph-alone` | `loom map` views and `--window` reads only |
| `graph-plus-rg` | `loom map` views, `--window`, `rg`, `fd` and file reads |
| `current-guidance` | the agent guidance loom ships today, unchanged |

All four modes use the same agent, model, token budget, tool budget and repository revision.

### Task set

Tasks are written before any run, in one YAML file. Fields:

```yaml
tasks:
  - id: rust-001                    # unique, stable
    repo: ~/holdout/project-a       # checkout path
    revision: 3f9c2e1a7b            # commit the checkout is reset to for every run
    class: find-callers             # find-callers | find-definition | impact | trace-flow | edit-site
    prompt: "List every function that calls Widget::new and the line of each call."
    expected:
      definitions: ["src/lib.rs#function:Widget::new"]
      sites: ["src/main.rs:7", "src/cli.rs:42"]
    ambiguous_ok:                   # answers accepted in place of a single expected one
      - ["src/a.rs#function:save", "src/b.rs#function:save"]
```

- `definitions` are node ids the answer must name. `sites` are `path:line` pairs it must cite.
- `ambiguous_ok` lists sets an answer may give when the language cannot decide between them.
  An answer naming every member of a set scores as correct.
- Cover every task class for every language in the holdout set, at least five tasks per class.

### Running

- Blind the grader. Strip mode names, tool names and `loom map` output from each transcript's
  final answer before grading, and grade the final answer against `expected` only.
- Randomize. For every task, draw the mode order from a seeded shuffle, and record the seed
  with the results.
- Repeat. Run every (task, mode) pair at least five times with fresh agent sessions, and
  reset the repository to `revision` before each run.
- Record per run: task id, mode, repeat index, order position, correctness, false claims,
  tokens, tool calls, wall time, and whether the run hit its budget.

## Metrics and stratification

Fixture and holdout evaluation record, per dialect: `declaration_recall`, `target_precision`,
`target_recall`, `unresolved_rate`, `ambiguous_rate`, `false_high_confidence` and
`impact_false_negatives` by depth. Agent comparison records, per mode: task success rate,
false-claim rate, tokens and tool calls per task, and time per task.

Report every number stratified by each of:

- language: the dialect id;
- task class: the `class` field of the task set;
- coverage tier: the coverage of the files the task touches, from the census (full, partial,
  lexical only);
- freshness: the snapshot state the run read (current, stale, or never built).

Report a stratum only with its sample size. A stratum with fewer than five tasks is listed
without a rate.

## Release rule

The thresholds in `loom/eval/edge-quality-thresholds.yaml` were published on 2026-09-30,
before any holdout project was examined. They are never changed after holdout results exist.
Changing one starts a new publication: a new date in the file header, and holdouts that have
not yet been examined.

A dialect or resolver capability ships to agent guidance only when all of these hold:

1. Its labelled fixture passes every threshold (`every_dialect_meets_published_thresholds`).
2. Its holdout corpora pass every threshold under `loom map --eval-edges`.
3. In the agent comparison, `graph-plus-rg` does not regress against `rg-first` and
   `current-guidance` on task success or false-claim rate within the dialect's cohort, in any
   stratum that has enough tasks to report a rate.

A dialect that fails any condition stays out of agent guidance. Its graph views remain
available to an operator who asks for them.
