//! Per-dialect coverage: dialect breakdown, gaps, sizes and ambiguity.

use super::tests::{entry, graph, node};
use super::*;
use crate::context::extract::dialect::GrammarPack;
use crate::context::resolve::fixtures::unresolved_edge;
use crate::context::source_graph::{FileCoverage, SourceEdgeKind, SourceNode};

/// A file node whose span ends at `end_byte`, the size the report reads.
fn sized_node(path: &str, end_byte: usize) -> SourceNode {
    let mut file = node(path, path);
    file.span.end_byte = end_byte;
    file
}

fn lexical(detail: &str) -> FileCoverage {
    FileCoverage::LexicalOnly {
        detail: detail.to_string(),
    }
}

/// A `.rs` file (registered when the core pack is compiled), a `.java` file
/// (wave B) and a `.md` file (no dialect), each with a known size.
fn mixed_dialect_graph() -> ResolvedGraph {
    graph(vec![
        (
            "src/a.rs",
            entry(
                FileCoverage::Full,
                vec![sized_node("src/a.rs", 600)],
                vec![],
            ),
        ),
        (
            "src/Main.java",
            entry(
                lexical("gap"),
                vec![sized_node("src/Main.java", 300)],
                vec![],
            ),
        ),
        (
            "README.md",
            entry(
                lexical("unsupported"),
                vec![sized_node("README.md", 100)],
                vec![],
            ),
        ),
    ])
}

/// The java half of the mixed graph's report: registered when the wave B pack
/// is compiled, otherwise a gap with its `not compiled` reason.
fn assert_java_coverage(report: &CoverageReport) {
    let java = &report.by_dialect["java"];
    assert_eq!(
        (java.files, java.bytes, java.symbol_level_bytes),
        (1, 300, 0)
    );
    if GrammarPack::WaveB.compiled() {
        assert_eq!(java.extractor, "registered");
        assert!(java.capabilities.is_some());
    } else {
        assert_eq!(java.extractor, "pack source-graph-wave-b not compiled");
        assert!(java.capabilities.is_none());
        assert_eq!(
            report.gaps,
            [GapCoverage {
                dialect: "java".to_string(),
                detail: "grammar pack source-graph-wave-b not compiled in (java)".to_string(),
                files: 1,
                bytes: 300,
            }]
        );
        assert!(report
            .to_string()
            .ends_with("; gaps: java 1 file (pack source-graph-wave-b not compiled)"));
    }
}

#[test]
fn a_mixed_graph_reports_dialect_files_bytes_gaps_and_unsupported() {
    let report = CoverageReport::of(&mixed_dialect_graph());

    assert_eq!((report.files, report.bytes), (3, 1000));
    assert_eq!(
        (report.symbol_level_files, report.symbol_level_bytes),
        (1, 600)
    );
    assert_eq!(
        (report.unsupported_files, report.unsupported_bytes),
        (1, 100)
    );
    let dialects: Vec<&str> = report.by_dialect.keys().map(String::as_str).collect();
    assert_eq!(dialects, ["java", "rust"], "the .md file joins no dialect");

    let rust = &report.by_dialect["rust"];
    assert_eq!(
        (rust.files, rust.bytes, rust.symbol_level_bytes),
        (1, 600, 600)
    );
    assert_eq!(rust.files_by_status.get("full"), Some(&1));
    if GrammarPack::Core.compiled() {
        assert_eq!(rust.extractor, "registered");
        assert!(rust.capabilities.is_some_and(|caps| caps.declarations));
    }

    assert_java_coverage(&report);
}

#[test]
fn display_adds_the_byte_share_and_drops_it_when_there_are_no_bytes() {
    let sized = CoverageReport::of(&mixed_dialect_graph()).to_string();
    assert!(
        sized.contains("33% of files, 60% of bytes symbol-level"),
        "byte share missing: {sized}"
    );
    assert!(!sized.contains('\n'));

    let empty_spans = graph(vec![(
        "src/a.rs",
        entry(
            FileCoverage::Full,
            vec![node("src/a.rs", "src/a.rs")],
            vec![],
        ),
    )]);
    let rendered = CoverageReport::of(&empty_spans).to_string();
    assert!(rendered.contains("100% symbol-level"), "{rendered}");
    assert!(!rendered.contains("of bytes"), "{rendered}");
}

#[test]
fn an_oversized_file_reports_its_own_size_not_its_span() {
    let g = graph(vec![(
        "src/huge.rs",
        entry(
            FileCoverage::Oversized {
                bytes: 5_000_000,
                limit: 1_000_000,
            },
            vec![sized_node("src/huge.rs", 12)],
            vec![],
        ),
    )]);

    let report = CoverageReport::of(&g);
    assert_eq!(report.bytes, 5_000_000);
    assert_eq!(report.symbol_level_bytes, 0);
    assert_eq!(report.by_dialect["rust"].bytes, 5_000_000);
}

#[test]
fn ambiguous_edges_count_only_edges_with_candidates() {
    let ambiguous = unresolved_edge("a#file:", SourceEdgeKind::Calls, "twin")
        .with_candidates(vec!["x#fn:twin".to_string(), "y#fn:twin".to_string()]);
    let plain = unresolved_edge("a#file:", SourceEdgeKind::Calls, "mystery");
    let g = graph(vec![(
        "src/a.rs",
        entry(
            FileCoverage::Full,
            vec![sized_node("src/a.rs", 40)],
            vec![ambiguous, plain],
        ),
    )]);

    let report = CoverageReport::of(&g);
    let rust = &report.by_dialect["rust"];
    assert_eq!(rust.ambiguous_edges, 1);
    assert_eq!(rust.unresolved_edges, 2);
    assert_eq!(rust.edges_by_provenance.get("syntax"), Some(&2));
}

#[test]
fn more_than_two_gap_dialects_collapse_to_a_count() {
    let g = graph(vec![
        (
            "A.java",
            entry(lexical("gap"), vec![sized_node("A.java", 1)], vec![]),
        ),
        (
            "B.cs",
            entry(lexical("gap"), vec![sized_node("B.cs", 1)], vec![]),
        ),
        (
            "c.rb",
            entry(lexical("gap"), vec![sized_node("c.rb", 1)], vec![]),
        ),
    ]);

    let report = CoverageReport::of(&g);
    if GrammarPack::WaveB.compiled() {
        for id in ["java", "csharp", "ruby"] {
            assert_eq!(report.by_dialect[id].extractor, "registered");
        }
    } else {
        assert_eq!(report.gaps.len(), 3);
        assert!(report.to_string().ends_with("; gaps: 3 dialects (3 files)"));
    }
}
