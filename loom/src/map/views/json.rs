//! Machine-readable pieces shared by every map view: the schema tag, the
//! coverage footer and the display-safe string helper. Each view builds its own
//! JSON object beside its text form.

use serde_json::{json, Value};

use crate::context::graph_store::ResolvedGraph;
use crate::context::{CoverageReport, ResolutionStats};

/// The `"schema"` value of a `loom map --json` payload.
pub const MAP_SCHEMA: &str = "loom-map/2";

pub fn footer_json(graph: &ResolvedGraph, stats: &ResolutionStats) -> Value {
    let coverage = CoverageReport::of(graph);
    json!({
        "files": coverage.files,
        "files_by_status": coverage.files_by_status,
        "symbol_level_files": coverage.symbol_level_files,
        "nodes": coverage.nodes,
        "edges": coverage.edges,
        "edges_by_provenance": coverage.edges_by_provenance,
        "unresolved_edges": coverage.unresolved_edges,
        "bytes": coverage.bytes,
        "symbol_level_bytes": coverage.symbol_level_bytes,
        "unsupported_files": coverage.unsupported_files,
        "unsupported_bytes": coverage.unsupported_bytes,
        "by_dialect": coverage.by_dialect,
        "gaps": coverage.gaps,
        "base_revision": safe(&coverage.base_revision),
        "overlaid_files": coverage.overlaid_files,
        "resolution": {
            "retargeted": stats.retargeted,
            "ambiguous": stats.ambiguous,
            "unresolved": stats.unresolved,
            "by_provenance": stats.by_provenance,
        },
    })
}

pub(super) fn safe(value: &str) -> String {
    crate::context::untrusted::inline_safe(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::freshness::GraphState;
    use crate::map::views::snapshot::SnapshotIdentity;
    use crate::map::views::tests::{impact_chain_graph, view_options};
    use crate::map::views::{
        callees_json, callers_json, find_all_json, impact_json, json_payload, outline_json,
        references_json, ViewContext,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use tempfile::TempDir;

    fn round_trip(value: Value) -> Value {
        serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap()
    }

    fn assert_keys(value: &Value, expected: &[&str]) {
        let actual = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected.iter().copied().collect::<BTreeSet<_>>());
    }

    fn assert_filters(value: &Value, applied: &str) {
        assert_keys(value, &["path", "lang"]);
        for filter in [&value["path"], &value["lang"]] {
            assert_keys(filter, &["value", "applied", "filtered_out"]);
            assert_eq!(filter["applied"], applied);
        }
    }

    #[test]
    fn outline_and_find_all_key_sets_are_stable() {
        let graph = impact_chain_graph();
        let root = TempDir::new().unwrap();
        let opts = view_options(Vec::new());
        let outline = round_trip(outline_json(&graph, root.path(), "src/a.rs"));
        let missing = round_trip(outline_json(&graph, root.path(), "src/missing.rs"));
        let find_all = round_trip(find_all_json(&graph, "foo", &opts));

        assert_keys(&outline, &["path", "match", "coverage", "symbols"]);
        assert_keys(&outline["coverage"], &["status"]);
        assert_keys(
            &outline["symbols"][0],
            &["id", "kind", "scope", "line_start", "line_end", "signature"],
        );
        assert_keys(&missing, &["path", "error", "match"]);
        assert_keys(
            &find_all,
            &[
                "symbol",
                "match",
                "exact",
                "matches",
                "suppressed_starts",
                "suppressed",
                "filters",
            ],
        );
        assert_filters(&find_all["filters"], "scan");
        assert_keys(
            &find_all["matches"][0],
            &["id", "kind", "path", "line_start", "coverage_status"],
        );
    }

    #[test]
    fn impact_key_set_is_stable() {
        let graph = impact_chain_graph();
        let root = TempDir::new().unwrap();
        let opts = view_options(Vec::new());
        let impact = round_trip(impact_json(&graph, root.path(), "foo", &opts));

        assert_keys(
            &impact,
            &[
                "match",
                "start_ids",
                "depth",
                "kinds",
                "hits",
                "suppressed_starts",
                "suppressed",
                "filters",
            ],
        );
        assert_filters(&impact["filters"], "display");
        assert_keys(
            &impact["hits"][0],
            &[
                "id",
                "kind",
                "path",
                "depth",
                "min_confidence",
                "weakest_provenance",
                "weakest_kind",
                "via_candidates",
            ],
        );
    }

    #[test]
    fn neighbor_key_sets_are_stable() {
        let graph = impact_chain_graph();
        let root = TempDir::new().unwrap();
        let opts = view_options(Vec::new());
        let callers = round_trip(callers_json(&graph, root.path(), "foo", &opts));
        let callees = round_trip(callees_json(&graph, root.path(), "bar", &opts));
        let references = round_trip(references_json(&graph, root.path(), "foo", &opts));

        for value in [&callers, &callees, &references] {
            assert_keys(
                value,
                &[
                    "match",
                    "start_ids",
                    "neighbors",
                    "suppressed_starts",
                    "suppressed",
                    "filters",
                ],
            );
            assert_filters(&value["filters"], "display");
        }
        for value in [&callers, &callees] {
            assert_keys(
                &value["neighbors"][0],
                &[
                    "id",
                    "kind",
                    "path",
                    "edge_kind",
                    "confidence",
                    "provenance",
                    "line_start",
                    "symbol",
                    "site_path",
                    "sites",
                    "candidate_of",
                ],
            );
        }
        assert_keys(
            &callers["neighbors"][0]["sites"][0],
            &["line_start", "line_end", "start_byte", "end_byte"],
        );
        assert!(callers["neighbors"][0]["candidate_of"].is_null());
    }

    #[test]
    fn footer_key_set_is_stable() {
        let graph = impact_chain_graph();
        let footer = round_trip(footer_json(&graph, &ResolutionStats::default()));

        assert_keys(
            &footer,
            &[
                "files",
                "files_by_status",
                "symbol_level_files",
                "nodes",
                "edges",
                "edges_by_provenance",
                "unresolved_edges",
                "bytes",
                "symbol_level_bytes",
                "unsupported_files",
                "unsupported_bytes",
                "by_dialect",
                "gaps",
                "base_revision",
                "overlaid_files",
                "resolution",
            ],
        );
        assert!(footer["by_dialect"].is_object());
        assert!(footer["gaps"].is_array());
        let rust = &footer["by_dialect"]["rust"];
        assert_keys(
            rust,
            &[
                "files",
                "bytes",
                "symbol_level_files",
                "symbol_level_bytes",
                "files_by_status",
                "edges_by_provenance",
                "unresolved_edges",
                "ambiguous_edges",
                "extractor",
                "capabilities",
            ],
        );
        assert_keys(
            &footer["resolution"],
            &["retargeted", "ambiguous", "unresolved", "by_provenance"],
        );
    }

    #[test]
    fn payload_carries_schema_and_snapshot_identity() {
        let graph = impact_chain_graph();
        let root = TempDir::new().unwrap();
        let stats = ResolutionStats::default();
        let snapshot = SnapshotIdentity {
            state: GraphState::Stale,
            base_revision: "abcdef0123456789".to_string(),
            overlay: Some(("local".to_string(), "tree".to_string())),
            generation: "0123456789abcdef".to_string(),
            built_at: None,
            persisted: true,
            schema_version: 2,
            extractors: BTreeMap::new(),
        };
        let ctx = ViewContext {
            graph: &graph,
            project_root: root.path(),
            stats: &stats,
            snapshot: &snapshot,
        };

        let payload = round_trip(json_payload(&ctx, serde_json::Map::new()));

        assert_keys(&payload, &["schema", "snapshot", "views", "coverage"]);
        assert_eq!(payload["schema"], "loom-map/2");
        assert_keys(
            &payload["snapshot"],
            &[
                "state",
                "base_revision",
                "overlay",
                "generation",
                "built_at",
                "persisted",
                "schema_version",
                "extractors",
            ],
        );
        assert_eq!(payload["snapshot"]["state"], "stale");
        assert_keys(&payload["snapshot"]["overlay"], &["plan", "stage"]);
        assert!(payload["snapshot"]["built_at"].is_null());
    }
}
