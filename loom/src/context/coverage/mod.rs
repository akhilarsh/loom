//! Honest coverage reporting over a [`ResolvedGraph`].
//!
//! The source graph never hides a degraded file: an unsupported language, an
//! oversized file, and a parse error all keep their file-level node and their
//! [`FileCoverage`](crate::context::source_graph::FileCoverage) tag. This
//! module turns that per-file honesty into a single summary that preserves
//! it — every status and every edge provenance is counted and shown, never
//! filtered down to "the good parts".
//!
//! The report keeps four questions apart: which files exist (files and
//! bytes), which parsed (status), which dialect and extractor applied
//! ([`DialectCoverage`]), and how edges resolved (provenance, unresolved,
//! ambiguous). Dialects the build cannot extract are listed as gaps.

mod dialects;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_dialects;

use std::collections::BTreeMap;
use std::fmt;

use crate::context::graph_store::ResolvedGraph;

pub use dialects::{DialectCoverage, GapCoverage};

/// A summary of a [`ResolvedGraph`]'s coverage.
///
/// Counts every file the graph knows about, including files that failed to
/// parse or exceeded the size cap — those are reported, never dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageReport {
    /// Files present in the graph.
    pub files: usize,
    /// File count per `FileCoverage::status()` value, e.g. "full" -> 388.
    pub files_by_status: BTreeMap<&'static str, usize>,
    /// Files whose coverage reports symbol-level extraction (`FileCoverage::has_symbols`).
    pub symbol_level_files: usize,
    /// Bytes over every file: the file node's end offset, or an oversized
    /// file's own size.
    pub bytes: usize,
    /// Bytes of the files counted in `symbol_level_files`.
    pub symbol_level_bytes: usize,
    /// Files whose extension no dialect claims.
    pub unsupported_files: usize,
    pub unsupported_bytes: usize,
    pub nodes: usize,
    pub edges: usize,
    /// Edge count per `EdgeProvenance::as_str()` value, e.g. "structural" -> 6100.
    pub edges_by_provenance: BTreeMap<&'static str, usize>,
    /// Edges still pointing at `UNRESOLVED_TARGET`.
    pub unresolved_edges: usize,
    /// One entry per dialect with at least one file, keyed by dialect id.
    pub by_dialect: BTreeMap<String, DialectCoverage>,
    /// Dialects the build knows but cannot extract.
    pub gaps: Vec<GapCoverage>,
    /// Revision the underlying base layer describes; empty when there is none
    /// (an overlay-only view before any base was ever published).
    pub base_revision: String,
    /// Files this view's overlay shadowed over the base — non-zero means the
    /// report describes a stage-local view, not the base alone.
    pub overlaid_files: usize,
}

impl CoverageReport {
    /// Summarise a resolved graph. Counts every file, including degraded ones.
    pub fn of(graph: &ResolvedGraph) -> Self {
        let mut files_by_status: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut symbol_level_files = 0usize;
        let mut bytes = 0usize;
        let mut symbol_level_bytes = 0usize;

        for entry in graph.files.values() {
            let file_bytes = dialects::file_bytes(entry);
            bytes += file_bytes;
            *files_by_status.entry(entry.coverage.status()).or_insert(0) += 1;
            if entry.coverage.has_symbols() {
                symbol_level_files += 1;
                symbol_level_bytes += file_bytes;
            }
        }

        let (edges_by_provenance, unresolved_edges) = edge_totals(graph);

        let dialects::DialectBreakdown {
            by_dialect,
            gaps,
            unsupported_files,
            unsupported_bytes,
        } = dialects::breakdown(graph);

        CoverageReport {
            files: graph.files.len(),
            files_by_status,
            symbol_level_files,
            bytes,
            symbol_level_bytes,
            unsupported_files,
            unsupported_bytes,
            nodes: graph.node_count(),
            edges: graph.edge_count(),
            edges_by_provenance,
            unresolved_edges,
            by_dialect,
            gaps,
            base_revision: graph.base_revision.clone(),
            overlaid_files: graph.overlaid.len(),
        }
    }

    /// Fraction of files that got symbol-level extraction, in `0.0..=1.0`.
    /// Returns 0.0 for an empty graph - never divides by zero.
    pub fn symbol_level_fraction(&self) -> f32 {
        if self.files == 0 {
            return 0.0;
        }
        self.symbol_level_files as f32 / self.files as f32
    }

    /// The symbol-level share for the footer: the file share alone when the
    /// graph has no bytes to divide, otherwise the file and the byte share.
    fn symbol_level_share(&self) -> String {
        let files = render_percent(self.symbol_level_fraction());
        if self.bytes == 0 {
            return files;
        }
        let bytes = render_percent(self.symbol_level_bytes as f32 / self.bytes as f32);
        format!("{files} of files, {bytes} of bytes")
    }

    /// The `; gaps: ...` footer clause, or an empty string without gaps. Names
    /// each gap dialect and why it is a gap; more than two collapse to a count.
    fn gaps_clause(&self) -> String {
        match self.gaps.as_slice() {
            [] => String::new(),
            gaps if gaps.len() > 2 => {
                let files: usize = gaps.iter().map(|gap| gap.files).sum();
                format!("; gaps: {} dialects ({files} files)", gaps.len())
            }
            gaps => {
                let named = gaps
                    .iter()
                    .map(|gap| {
                        let reason = self
                            .by_dialect
                            .get(&gap.dialect)
                            .map_or(gap.detail.as_str(), |dialect| dialect.extractor.as_str());
                        let noun = if gap.files == 1 { "file" } else { "files" };
                        format!("{} {} {noun} ({reason})", gap.dialect, gap.files)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("; gaps: {named}")
            }
        }
    }

    /// First 8 characters of [`Self::base_revision`], or `"none"` when empty.
    fn short_base_revision(&self) -> &str {
        if self.base_revision.is_empty() {
            "none"
        } else {
            self.base_revision.get(..8).unwrap_or(&self.base_revision)
        }
    }
}

/// Edge count per provenance and the number of edges still unresolved, over
/// every edge of `graph`.
fn edge_totals(graph: &ResolvedGraph) -> (BTreeMap<&'static str, usize>, usize) {
    let mut edges_by_provenance: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut unresolved_edges = 0usize;

    for edge in graph.edges() {
        *edges_by_provenance
            .entry(edge.provenance.as_str())
            .or_insert(0) += 1;
        if edge.is_unresolved() {
            unresolved_edges += 1;
        }
    }
    (edges_by_provenance, unresolved_edges)
}

/// Render a `0.0..=1.0` fraction as a whole-number percentage, except a
/// non-zero fraction that would otherwise round down to `0%` renders as `<1%`
/// — "0%" reads as "none had symbols", which is false.
fn render_percent(fraction: f32) -> String {
    let percent = (fraction * 100.0).round();
    if fraction > 0.0 && percent < 1.0 {
        "<1%".to_string()
    } else {
        format!("{percent}%")
    }
}

/// Render a `key count` list from a status/provenance breakdown map, or an
/// empty string when the map is empty - callers decide whether to wrap the
/// result in a parenthetical group.
fn render_breakdown(counts: &BTreeMap<&'static str, usize>) -> String {
    counts
        .iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for CoverageReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base_suffix = format!(
            " - base {}, {} overlaid",
            self.short_base_revision(),
            self.overlaid_files
        );

        if self.files == 0 {
            return write!(f, "coverage: 0 files - no files indexed{base_suffix}");
        }

        let status_group = render_breakdown(&self.files_by_status);
        let status_group = if status_group.is_empty() {
            String::new()
        } else {
            format!(" ({status_group})")
        };

        let edges_group = render_breakdown(&self.edges_by_provenance);
        let edges_group = if edges_group.is_empty() {
            String::new()
        } else {
            format!(" ({edges_group}; {} unresolved)", self.unresolved_edges)
        };

        write!(
            f,
            "coverage: {} files{status_group} - {} symbol-level - {} nodes, {} edges{edges_group}{base_suffix}{}",
            self.files,
            self.symbol_level_share(),
            self.nodes,
            self.edges,
            self.gaps_clause()
        )
    }
}
