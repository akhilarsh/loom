# Automatic Knowledge Source Graph Followups

> Knowledge-plan followups: retrieval gap, stopwording

## Open After PLAN-automatic-knowledge-and-source-graph (2026-08-18)

**Whole-file read ahead of the size cap.** `context/refresh/source_graph.rs:228` does
`fs::read` on every tracked file BEFORE `extract_file` applies the 512 KiB
`MAX_EXTRACTED_FILE_BYTES` cap, so the cap bounds parsing but not allocation, and the
daemon spikes to the size of the largest tracked blob on every merge reconcile.
Deliberately not fixed at the quality gate: `FileExtraction::file_level`
(`extract/mod.rs:103`) needs the BYTES to build the file node's span, so avoiding the
read means changing the oversized node's span semantics or threading a streamed line
count through the extractor API — a hot-path refactor. Peak is one file at a time and
`EXCLUDED_ROOTS` already skips `target/` and `node_modules/`, so the realistic worst
case is a transient spike, not corruption.

**Four production-dead `KnowledgeDir` methods.** Deleting `loom knowledge show`/`list`
orphaned part of the read/replace side: `read` (`dir.rs:120`), `append` (`dir.rs:127`),
`read_index` (`dir.rs:160`), and `replace_section` (`dir.rs:136`, the
`KnowledgeFile`-keyed variant) have no non-test callers, and all are `pub` on a `pub`
type so clippy cannot see them. They were kept because ~15 tests in `tests_dir.rs`
exercise them against each other (append → read, replace_section → read), so deleting
the methods deletes most of that file's coverage. **Settle them deliberately in one
follow-up: either delete methods and tests together, or wire them to a real consumer.**
General rule: when a stage deletes a read-side CLI verb, audit every accessor that verb
was the last caller of — and when a brief justifies keeping a module by naming a
caller, check whether that caller is itself reachable. A wrapper is not a consumer. The
converse also held here: `loom knowledge replace-section` was restored as a live CLI
verb (`cli/types_memory.rs:19`, `cli/dispatch.rs:84-88`, `commands/knowledge/mod.rs:115`),
which revived two of the original six dead methods — `read_target` (`dir.rs:176`, now
called at `commands/knowledge/mod.rs:126`) and `replace_section_target` (`dir.rs:212`,
now called at `commands/knowledge/mod.rs:130`). A dead-accessor list like this one is
only true against one revision; re-check it before trusting it.

**Plan-key normalisation on the writer side.** `delivery::plan_key` resolves both a blank
`plan_id` in `.loom/work/config.toml` and a stage record with no plan to `"default"`;
`MergeLifecycle`'s writer side does not normalise identically. Silent by construction —
see `mistakes/writer-reader-address.md`.

**A permission deny now reaches child processes.** The knowledge tree is denied to the
agent AND to the `loom` binary the doctrine tells agents to use. See Part C of the
pending-knowledge document, and `../mistakes/sandbox-write-rules-inert.md` for the history.

**`fs/permissions/constants.rs`** still declares `LOOM_PERMISSIONS_WORKTREE` with
`Write(.loom/work/**)` / `Bash(loom *)` rules that read like a blanket grant but have no real
consumers, and `Write(path)` rules are inert anyway. A documented fossil.

## A Never-Built Source Graph Reads as Stale

`Freshness::never_built` (`context/freshness.rs`) sets `revision` to the empty string and
`stale: true`. The Knowledge Brief header (`orchestrator/signals/format/brief.rs::freshness_word`)
therefore prints `Semantic: stale` for a graph that was never built, the same word it prints for a
graph whose `HEAD` moved. `loom knowledge context` adds the detail
(`stale (source graph not built; …)`); the prompt-hook brief prints only the word.
`degraded_reason` (`context/retrieve/graph.rs:143-151`) returns `None` for the never-built case,
so no `DEGRADED:` banner appears either.

`loom map` itself works in a checkout that never ran `loom init`: `WorkDir::new` falls back to
`.loom/work`, and `refresh/tests_snapshot.rs::map_answers_in_a_checkout_that_never_ran_init`
pins it.

The rebuild trigger already fires on this state. `commands::hook::reconcile_graph::spawn_if_needed`
spawns on `stale || degraded`, so a never-built graph starts a detached full-repository build. The
spawn happens at most once per `reconcile_debounce_secs` (600 s) when `.loom/cache/context-v1`
exists or `LOOM_WORK_DIR` is set. While the build keeps failing it retries at that fixed interval,
with no backoff.

This is the shape recorded in
[visibility-and-reachability.md](../mistakes/visibility-and-reachability.md): one value that means
two things cannot gate a claim about either. Stage `map-api-freshness` of
`doc/plans/PLAN-source-graph-mechanism.md` makes `current | stale | never built | unavailable`
explicit and stops the prompt hook from rebuilding a never-built graph.

## Stopwording drops the words a natural-language source-graph question is asked in (2026-09-06)

**Observed:** `loom knowledge context --query "how is the source graph refreshed after a commit"` (`--explain`) drops `source`, `graph`, `commit` as corpus-ubiquitous and returns three unrelated low-confidence chunks; `architecture/source-graph.md#lifecycle-who-builds-it-and-when` — the section that answers it — is not in the pack. The same question phrased with a symbol (`reconcile_base`) ranks the right function first. `loom knowledge eval` still passes at precision@5 = 1.00 because its cases are symbol- or path-shaped.

**Why it matters:** the knowledge-first doctrine now tells every session to pull a question instead of reading; a pull that misses on the project's own vocabulary sends the reader back to paging files.

**Where to look:** `context/rank/corpus/stopwords.rs` (the corpus-derived stopword threshold and its rescue floor, described in `architecture/context-retrieval-corpus.md#corpus-derived-query-stopwording-with-a-rescue-floor`), and `loom/eval/retrieval-cases.yaml`, which has no natural-language lifecycle case. A first step is adding that case so the gap is measured before the threshold is tuned.

## An Unparseable Base Layer Wedges the Graph Cache

`read_layer` (`context/graph_store/mod.rs`) returns `Err` when a layer file fails to deserialize. `ensure_base` (`context/refresh/snapshot.rs`) calls `load_base(..)?` before its `layer_is_current` check and its delete, so a corrupt base for `HEAD` never reaches the rebuild. The snapshot reports `Unavailable` and marks the semantic layer stale. `resolve_scope_layers` uses `load_newest_base()?`, so an unparseable newest base also blocks building any other base until it is pruned or removed by hand. Retrieval (`context/retrieve/graph.rs::load_resolved_graph`) degrades to an empty graph with no banner, and `build_worktree_graph` (`context/worktree_graph.rs`) propagates the error into `reachable` checks and impact-selected tests. No test covers a stored layer that fails to deserialize. The graph layer has no schema version: the only versioning is the per-node `parser_version`. Stage `graph-contract` of `doc/plans/PLAN-source-graph-mechanism.md` adds `GRAPH_SCHEMA_VERSION` and treats a corrupt layer as absent.

## Worktree Graphs Trust Stale Extractor Output

`build_worktree_graph` (`context/worktree_graph.rs`) loads the nearest published base and re-extracts only the files changed in the worktree. It never compares a base entry's `parser_version` with the current extractor identity. A base written by an older extractor therefore serves old-shape entries for every unchanged file, mixed with new-shape entries for the changed ones, in `reachable` checks and impact-selected tests. Stage `graph-contract` of `doc/plans/PLAN-source-graph-mechanism.md` re-extracts mismatched entries.
