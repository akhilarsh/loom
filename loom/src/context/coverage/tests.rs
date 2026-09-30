use super::*;
use crate::context::graph_store::FileEntry;
use crate::context::resolve::fixtures::{local_edge, unresolved_edge};
use crate::context::source_graph::{
    FileCoverage, NodeLanguage, SourceEdge, SourceEdgeKind, SourceNode, SourceNodeKind, Span,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub(super) fn node(id: &str, path: &str) -> SourceNode {
    SourceNode {
        id: id.to_string(),
        kind: SourceNodeKind::File,
        path: PathBuf::from(path),
        scope: Vec::new(),
        span: Span::default(),
        signature: String::new(),
        body_hash: "sha256:deadbeef".to_string(),
        language: NodeLanguage::Rust,
        parser_version: "test+v1".to_string(),
        coverage: FileCoverage::Full,
        symbol_key: String::new(),
    }
}

pub(super) fn entry(
    coverage: FileCoverage,
    nodes: Vec<SourceNode>,
    edges: Vec<SourceEdge>,
) -> FileEntry {
    FileEntry {
        content_hash: "sha256:abc".to_string(),
        nodes,
        edges,
        coverage,
        imports: Vec::new(),
    }
}

pub(super) fn graph(files: Vec<(&str, FileEntry)>) -> ResolvedGraph {
    let mut map = BTreeMap::new();
    for (path, entry) in files {
        map.insert(path.to_string(), entry);
    }
    ResolvedGraph {
        base_revision: "rev1".to_string(),
        overlaid: Default::default(),
        files: map,
    }
}

#[test]
fn counts_are_right_for_a_mixed_graph() {
    let full_edge = local_edge("a#file:", "b#file:", SourceEdgeKind::Calls, "f");
    let unresolved = unresolved_edge("a#file:", SourceEdgeKind::Calls, "mystery");

    let g = graph(vec![
        (
            "src/a.rs",
            entry(
                FileCoverage::Full,
                vec![node("src/a.rs", "src/a.rs")],
                vec![full_edge, unresolved],
            ),
        ),
        (
            "src/b.rs",
            entry(
                FileCoverage::Full,
                vec![node("src/b.rs", "src/b.rs")],
                vec![],
            ),
        ),
        (
            "vendor/lib.min.js",
            entry(
                FileCoverage::LexicalOnly {
                    detail: "no extractor".to_string(),
                },
                vec![node("vendor/lib.min.js", "vendor/lib.min.js")],
                vec![],
            ),
        ),
    ]);

    let report = CoverageReport::of(&g);

    assert_eq!(report.files, 3);
    assert_eq!(report.files_by_status.get("full"), Some(&2));
    assert_eq!(report.files_by_status.get("lexical-only"), Some(&1));
    assert_eq!(report.symbol_level_files, 2);
    assert_eq!(report.nodes, 3);
    assert_eq!(report.edges, 2);
    assert_eq!(report.edges_by_provenance.get("local-name"), Some(&1));
    assert_eq!(report.edges_by_provenance.get("syntax"), Some(&1));
    assert_eq!(report.unresolved_edges, 1);
    assert_eq!(report.base_revision, "rev1");
    assert_eq!(report.overlaid_files, 0);
}

#[test]
fn a_parse_error_file_is_reported_not_hidden() {
    // Regression: a graph containing a degraded file must still surface
    // it in `files_by_status` rather than silently dropping it.
    let g = graph(vec![(
        "src/broken.rs",
        entry(
            FileCoverage::ParseError {
                span: Span::default(),
                detail: "unexpected token".to_string(),
            },
            vec![node("src/broken.rs", "src/broken.rs")],
            vec![],
        ),
    )]);

    let report = CoverageReport::of(&g);

    assert_eq!(report.files, 1);
    assert_eq!(report.files_by_status.get("parse-error"), Some(&1));
    assert_eq!(report.symbol_level_files, 0);

    let rendered = report.to_string();
    assert!(
        rendered.contains("parse-error 1"),
        "display output missing parse-error group: {rendered}"
    );
}

#[test]
fn symbol_level_fraction_on_an_empty_graph_is_zero_and_does_not_panic() {
    let report = CoverageReport::default();
    assert_eq!(report.symbol_level_fraction(), 0.0);
    assert_eq!(
        report.to_string(),
        "coverage: 0 files - no files indexed - base none, 0 overlaid"
    );
}

#[test]
fn display_renders_the_documented_one_line_shape() {
    let g = graph(vec![
        (
            "src/a.rs",
            entry(
                FileCoverage::Full,
                vec![node("src/a.rs", "src/a.rs")],
                vec![],
            ),
        ),
        (
            "src/b.rs",
            entry(
                FileCoverage::LexicalOnly {
                    detail: "unsupported".to_string(),
                },
                vec![node("src/b.rs", "src/b.rs")],
                vec![local_edge(
                    "src/b.rs#file:",
                    "src/a.rs#file:",
                    SourceEdgeKind::References,
                    "thing",
                )],
            ),
        ),
    ]);

    let report = CoverageReport::of(&g);
    let rendered = report.to_string();

    assert!(rendered.starts_with("coverage: 2 files ("));
    assert!(rendered.contains("full 1"));
    assert!(rendered.contains("lexical-only 1"));
    assert!(rendered.contains("50% symbol-level"));
    assert!(rendered.contains("2 nodes, 1 edges (local-name 1; 0 unresolved)"));
    assert!(rendered.ends_with("- base rev1, 0 overlaid"));
    assert!(!rendered.ends_with('\n'));
}

#[test]
fn base_revision_and_overlaid_files_are_reported_and_the_revision_is_truncated() {
    let mut files = BTreeMap::new();
    files.insert(
        "src/a.rs".to_string(),
        entry(
            FileCoverage::Full,
            vec![node("src/a.rs", "src/a.rs")],
            vec![],
        ),
    );
    let mut overlaid = BTreeSet::new();
    overlaid.insert("src/a.rs".to_string());

    let g = ResolvedGraph {
        base_revision: "0123456789abcdef".to_string(),
        overlaid,
        files,
    };

    let report = CoverageReport::of(&g);
    assert_eq!(report.base_revision, "0123456789abcdef");
    assert_eq!(report.overlaid_files, 1);
    assert!(report.to_string().ends_with("- base 01234567, 1 overlaid"));
}

#[test]
fn symbol_level_percent_renders_sub_one_percent_as_lt_one() {
    let mut files = BTreeMap::new();
    files.insert(
        "src/a.rs".to_string(),
        entry(
            FileCoverage::Full,
            vec![node("src/a.rs", "src/a.rs")],
            vec![],
        ),
    );
    for i in 0..999 {
        files.insert(
            format!("src/gen_{i}.rs"),
            entry(
                FileCoverage::LexicalOnly {
                    detail: "unsupported".to_string(),
                },
                vec![node(&format!("src/gen_{i}.rs"), &format!("src/gen_{i}.rs"))],
                vec![],
            ),
        );
    }
    let g = ResolvedGraph {
        base_revision: String::new(),
        overlaid: Default::default(),
        files,
    };

    let report = CoverageReport::of(&g);
    assert_eq!(report.files, 1000);
    assert_eq!(report.symbol_level_files, 1);
    assert!(report.to_string().contains("<1% symbol-level"));
}
