# Automatic Knowledge Source Graph Followups

> Knowledge-plan followups: retrieval gap, stopwording

## Open Items From the Automatic-Knowledge Plan

**Whole-file read ahead of the size cap.** `read_bytes` (`context/refresh/source_graph/layer.rs`)
reads every tracked file with `read_bounded(.., usize::MAX)` before `extract_file` applies the
512 KiB `MAX_EXTRACTED_FILE_BYTES` cap. The cap bounds parsing but not allocation, so the daemon
spikes to the size of the largest tracked blob on every reconcile. `FileExtraction::file_level`
(`context/extract/mod.rs`) needs the bytes to build an oversized file node's span and its
whole-file `content_hash`. Bounding the read therefore means changing that node's span and hash
semantics, or threading a streamed line count through the extractor API. Peak is one file at a
time, and `EXCLUDED_ROOTS` skips `target/` and `node_modules/`, so the realistic worst case is a
transient spike.

**Three production-dead `KnowledgeDir` methods.** `append`, `read_index` and the
`KnowledgeFile`-keyed `replace_section` (`fs/knowledge/dir.rs`) have no non-test callers.
`read` is live (`commands/knowledge/bootstrap/mod.rs`), and so are `read_target` and
`replace_section_target` (`commands/knowledge/mod.rs`). All are `pub` on a `pub` type, so clippy
cannot flag them, and `tests_dir.rs` and `tests_dir_bugfixes.rs` exercise the dead three against
each other. Settle them in one change: delete the methods and their tests together, or wire them
to a real consumer. A dead-accessor list holds for one revision only; re-check it before acting.

**Plan-key normalisation on the writer side.** `delivery::plan_key_from` files a blank or absent
`plan_id` under `"default"`. `MergeLifecycle::plan_id` (`orchestrator/merge_lifecycle.rs`) reads
the raw config `plan_id` instead: an absent id skips overlay reconcile and discard, and a blank
one is used as is. Silent by construction; see `mistakes/writer-reader-address.md`.

**`LOOM_PERMISSIONS_WORKTREE` has no consumer.** The constant (`fs/permissions/constants.rs`)
lists worktree-shaped `Read(...)` rules and `Bash(loom *)`, but only its re-export in
`fs/permissions/mod.rs` and `constants_tests.rs` reference it; no settings writer uses it. Wire it
into the worktree settings or delete it with its tests.

## Stopwording Drops the Words a Natural-Language Source-Graph Question Is Asked In

**Observed:** `loom knowledge context --query "how is the source graph refreshed after a commit"`
returns low-confidence chunks unrelated to the question;
`architecture/source-graph.md#lifecycle-who-builds-it-and-when`, the section that answers it, is
not in the pack. Corpus-derived stopwording drops `source`, `graph` and `commit` as ubiquitous. The
same question phrased with a symbol (`reconcile_base`) ranks the right function first.

**Why it matters:** the knowledge-first doctrine tells every session to pull a question instead of
reading; a pull that misses on the project's own vocabulary sends the reader back to paging files.

**Where to look:** `context/rank/corpus/stopwords.rs` (the corpus-derived stopword threshold and
its rescue floor, described in
`architecture/context-retrieval-corpus.md#corpus-derived-query-stopwording-with-a-rescue-floor`),
and `loom/eval/retrieval-cases.yaml`. Its only natural-language case asks about hook
configuration, so `loom knowledge eval` does not measure this gap. A first step is adding a
lifecycle case so the gap is measured before the threshold is tuned.

## `loom knowledge eval` Fails Its Gates

Three causes:

- **The eval never refreshes the source graph.** `eval` (`commands/knowledge/eval.rs`) calls `retrieve_for_stage` for each case without first running `ensure_snapshot(.., SnapshotPolicy::LocalCurrent)`, which `loom map`'s `load_graph` does. After a commit the local overlay is stale, the source channel returns no nodes, and the three source-node cases fail, so the result swings between runs.
- **A moved section.** `genuine-win-delivery-epoch-suppression` expects `architecture/context-retrieval.md#delivery-records-and-epoch-suppression#0`; that section now lives in `architecture/context-retrieval-state.md`.
- **A case that needs re-judging.** `genuine-win-sandbox-settings-rules` expects two `mistakes/sandbox-and-settings.md` chunks that now rank 15th and 18th. Ahead of them are architecture sections that answer the query directly (`security-and-isolation.md` on write grants and on worktree isolation) and `sandbox/settings/policy.rs::sandbox_settings`. `architecture/context-retrieval-corpus.md#the-rescue-floor-and-the-thin-survivor-rule` quotes this query's words, so it ranks for the query too.

`precision_floor` (0.40) was calibrated at p@5 0.45. Re-measure with a binary built from the tree under evaluation: an older installed `loom` scores old retrieval code. The `retrieval-delivery` stage owns the eval files and changes ranking, so re-judge the cases after it merges.
