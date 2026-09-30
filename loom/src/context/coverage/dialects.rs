//! Per-dialect coverage: which dialect each file belongs to, whether an
//! extractor handled it, how many bytes it covers and how its edges resolved.
//!
//! A file's dialect and extractor status come through [`extractor_for`], the
//! same lookup `extract_file` uses, so the report can never disagree with what
//! extraction actually did.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::context::extract::dialect::DialectSpec;
use crate::context::extract::{extractor_for, registry, Capabilities, Lookup};
use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::source_graph::{FileCoverage, SourceNodeKind};

/// The `extractor` value of a dialect with a registered extractor.
const REGISTERED: &str = "registered";

/// Coverage of one dialect: every file whose extension the dialect table
/// assigns to it, whether or not an extractor handled them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DialectCoverage {
    pub files: usize,
    pub bytes: usize,
    pub symbol_level_files: usize,
    pub symbol_level_bytes: usize,
    /// File count per `FileCoverage::status()` value.
    pub files_by_status: BTreeMap<&'static str, usize>,
    /// Edge count per `EdgeProvenance::as_str()` value, over this dialect's files.
    pub edges_by_provenance: BTreeMap<&'static str, usize>,
    /// Edges of this dialect's files still pointing at `UNRESOLVED_TARGET`.
    pub unresolved_edges: usize,
    /// Edges of this dialect's files carrying a non-empty candidate set.
    pub ambiguous_edges: usize,
    /// `"registered"`, `"pack {feature} not compiled"` or `"no extractor"`.
    pub extractor: String,
    /// What the registered extractor emits; `None` for a gap dialect.
    pub capabilities: Option<Capabilities>,
}

/// A dialect the build knows but cannot extract, with the files it leaves at
/// file-level coverage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GapCoverage {
    pub dialect: String,
    /// The lookup's detail text, the same string a gap file's
    /// `FileCoverage::LexicalOnly` carries.
    pub detail: String,
    pub files: usize,
    pub bytes: usize,
}

/// The dialect-level half of a [`CoverageReport`](super::CoverageReport).
#[derive(Debug, Default)]
pub(super) struct DialectBreakdown {
    pub by_dialect: BTreeMap<String, DialectCoverage>,
    pub gaps: Vec<GapCoverage>,
    pub unsupported_files: usize,
    pub unsupported_bytes: usize,
}

/// Size of one file: an oversized file reports its own size, every other file
/// its file node's end offset (0 when the entry carries no file node).
pub(super) fn file_bytes(entry: &FileEntry) -> usize {
    if let FileCoverage::Oversized { bytes, .. } = &entry.coverage {
        return *bytes;
    }
    entry
        .nodes
        .iter()
        .find(|node| node.kind == SourceNodeKind::File)
        .map_or(0, |node| node.span.end_byte)
}

/// Group every file of `graph` by dialect. The registry is built once.
pub(super) fn breakdown(graph: &ResolvedGraph) -> DialectBreakdown {
    let extractors = registry();
    let mut out = DialectBreakdown::default();
    // Gap dialect id -> the lookup's detail, recorded on first sight.
    let mut gap_details: BTreeMap<&'static str, String> = BTreeMap::new();

    for (path, entry) in &graph.files {
        let bytes = file_bytes(entry);
        let (dialect, extractor, capabilities) = match extractor_for(&extractors, Path::new(path)) {
            Lookup::Unknown => {
                out.unsupported_files += 1;
                out.unsupported_bytes += bytes;
                continue;
            }
            Lookup::Extractor(found) => (
                found.dialect(),
                REGISTERED.to_string(),
                Some(found.capabilities()),
            ),
            Lookup::Gap { dialect, detail } => {
                gap_details.entry(dialect.id).or_insert(detail);
                (dialect, gap_extractor_status(dialect), None)
            }
        };

        let slot = out
            .by_dialect
            .entry(dialect.id.to_string())
            .or_insert_with(|| DialectCoverage {
                extractor,
                capabilities,
                ..DialectCoverage::default()
            });
        tally_file(slot, entry, bytes);
    }

    out.gaps = gap_coverage(gap_details, &out.by_dialect);
    out
}

/// The `extractor` value of a gap dialect: its pack is missing from the build,
/// or the pack is compiled and no extractor is registered.
fn gap_extractor_status(dialect: &DialectSpec) -> String {
    if dialect.pack.compiled() {
        "no extractor".to_string()
    } else {
        format!("pack {} not compiled", dialect.pack.feature())
    }
}

/// One [`GapCoverage`] per gap dialect that has at least one file, sized from
/// its [`DialectCoverage`] entry.
fn gap_coverage(
    gap_details: BTreeMap<&'static str, String>,
    by_dialect: &BTreeMap<String, DialectCoverage>,
) -> Vec<GapCoverage> {
    gap_details
        .into_iter()
        .filter_map(|(id, detail)| {
            let slot = by_dialect.get(id)?;
            Some(GapCoverage {
                dialect: id.to_string(),
                detail,
                files: slot.files,
                bytes: slot.bytes,
            })
        })
        .collect()
}

/// Add one file, its status and its edges to a dialect's totals.
fn tally_file(slot: &mut DialectCoverage, entry: &FileEntry, bytes: usize) {
    slot.files += 1;
    slot.bytes += bytes;
    if entry.coverage.has_symbols() {
        slot.symbol_level_files += 1;
        slot.symbol_level_bytes += bytes;
    }
    *slot
        .files_by_status
        .entry(entry.coverage.status())
        .or_insert(0) += 1;
    for edge in &entry.edges {
        *slot
            .edges_by_provenance
            .entry(edge.provenance.as_str())
            .or_insert(0) += 1;
        if edge.is_unresolved() {
            slot.unresolved_edges += 1;
        }
        if !edge.candidates.is_empty() {
            slot.ambiguous_edges += 1;
        }
    }
}
