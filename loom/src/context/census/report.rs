//! The census result: its JSON shape (`loom-census/1`) and its text form.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;

use crate::context::refresh::EXCLUDED_ROOTS;

/// Schema tag at the top of the JSON form.
pub const CENSUS_SCHEMA: &str = "loom-census/1";

/// How many unsupported extensions the text form lists.
const TOP_UNSUPPORTED: usize = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Count {
    pub files: u64,
    pub bytes: u64,
}

impl Count {
    pub(super) fn add_file(&mut self, bytes: u64) {
        self.files = self.files.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
    }

    fn merge(&mut self, other: Count) {
        self.files = self.files.saturating_add(other.files);
        self.bytes = self.bytes.saturating_add(other.bytes);
    }
}

/// Files and bytes per class. A file is in exactly one class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ClassTotals {
    pub eligible: Count,
    pub excluded: Count,
    pub vendored: Count,
    pub generated: Count,
    pub unsupported: Count,
}

impl ClassTotals {
    pub(super) fn merge(&mut self, other: &ClassTotals) {
        self.eligible.merge(other.eligible);
        self.excluded.merge(other.excluded);
        self.vendored.merge(other.vendored);
        self.generated.merge(other.generated);
        self.unsupported.merge(other.unsupported);
    }
}

/// Eligible files of one dialect inside one subproject.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DialectCoverage {
    pub files: u64,
    pub bytes: u64,
    /// Files whose symbols were extracted (`full` or `partial` coverage).
    pub symbol_files: u64,
    pub symbol_bytes: u64,
    pub parse_errors: u64,
    /// `available`, or `gap: <why>` when no extractor serves the dialect.
    pub extractor: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageSource {
    /// The resolved graph of the checkout `loom map` runs in.
    Graph,
    /// Extracted in memory for this run; nothing is cached.
    InMemory,
}

/// What the in-memory extraction of a root did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ParseStats {
    pub files_parsed: u64,
    pub bytes_read: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubprojectCensus {
    /// Repo-relative directory of the manifest, `.` for the root.
    pub path: String,
    pub totals: ClassTotals,
    pub by_dialect: BTreeMap<String, DialectCoverage>,
    pub unsupported_by_extension: BTreeMap<String, Count>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootCensus {
    pub root: String,
    pub coverage_source: CoverageSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse: Option<ParseStats>,
    pub totals: ClassTotals,
    pub subprojects: Vec<SubprojectCensus>,
}

impl RootCensus {
    /// Eligible files and bytes whose symbols were extracted.
    pub fn symbol_level(&self) -> Count {
        let mut count = Count::default();
        for coverage in self
            .subprojects
            .iter()
            .flat_map(|subproject| subproject.by_dialect.values())
        {
            count.merge(Count {
                files: coverage.symbol_files,
                bytes: coverage.symbol_bytes,
            });
        }
        count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusReport {
    pub roots: Vec<RootCensus>,
    pub totals: ClassTotals,
}

impl Serialize for CensusReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("CensusReport", 3)?;
        state.serialize_field("schema", CENSUS_SCHEMA)?;
        state.serialize_field("roots", &self.roots)?;
        state.serialize_field("totals", &self.totals)?;
        state.end()
    }
}

fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "n/a".to_string();
    }
    format!("{:.1}%", part as f64 * 100.0 / whole as f64)
}

/// Every class beside the eligible denominators, with the symbol-level share
/// of those denominators.
fn totals_line(totals: &ClassTotals, symbol: Count) -> String {
    let eligible = totals.eligible;
    format!(
        "eligible {} files {} bytes (symbol-level {}/{} files {}, {}/{} bytes {}); \
         excluded {} files {} bytes; vendored {} files {} bytes; \
         generated {} files {} bytes; unsupported {} files {} bytes",
        eligible.files,
        eligible.bytes,
        symbol.files,
        eligible.files,
        percent(symbol.files, eligible.files),
        symbol.bytes,
        eligible.bytes,
        percent(symbol.bytes, eligible.bytes),
        totals.excluded.files,
        totals.excluded.bytes,
        totals.vendored.files,
        totals.vendored.bytes,
        totals.generated.files,
        totals.generated.bytes,
        totals.unsupported.files,
        totals.unsupported.bytes,
    )
}

impl fmt::Display for RootCensus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.coverage_source, self.parse) {
            (CoverageSource::InMemory, Some(parse)) => writeln!(
                f,
                "root {} [in-memory extraction: {} files parsed, {} bytes read, {} ms]",
                self.root, parse.files_parsed, parse.bytes_read, parse.elapsed_ms
            )?,
            _ => writeln!(f, "root {} [resolved graph]", self.root)?,
        }
        for subproject in &self.subprojects {
            for (dialect, coverage) in &subproject.by_dialect {
                writeln!(
                    f,
                    "  {}  {}  files {}  bytes {}  symbol-level {}/{} files {}, {}/{} bytes {}  \
                     parse-errors {}  extractor {}",
                    subproject.path,
                    dialect,
                    coverage.files,
                    coverage.bytes,
                    coverage.symbol_files,
                    coverage.files,
                    percent(coverage.symbol_files, coverage.files),
                    coverage.symbol_bytes,
                    coverage.bytes,
                    percent(coverage.symbol_bytes, coverage.bytes),
                    coverage.parse_errors,
                    coverage.extractor,
                )?;
            }
        }
        Ok(())
    }
}

impl CensusReport {
    /// Unsupported extensions across every root, largest bytes first.
    fn top_unsupported(&self) -> Vec<(String, Count)> {
        let mut merged: BTreeMap<&str, Count> = BTreeMap::new();
        for (extension, count) in self
            .roots
            .iter()
            .flat_map(|root| &root.subprojects)
            .flat_map(|subproject| &subproject.unsupported_by_extension)
        {
            merged.entry(extension).or_default().merge(*count);
        }
        let mut ranked: Vec<(String, Count)> = merged
            .into_iter()
            .map(|(extension, count)| (extension.to_string(), count))
            .collect();
        ranked.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(TOP_UNSUPPORTED);
        ranked
    }

    fn symbol_level(&self) -> Count {
        let mut count = Count::default();
        for root in &self.roots {
            count.merge(root.symbol_level());
        }
        count
    }
}

impl fmt::Display for CensusReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for root in &self.roots {
            write!(f, "{root}")?;
            if self.roots.len() > 1 {
                writeln!(
                    f,
                    "  root totals: {}",
                    totals_line(&root.totals, root.symbol_level())
                )?;
            }
        }
        writeln!(
            f,
            "totals: {}",
            totals_line(&self.totals, self.symbol_level())
        )?;
        writeln!(f, "excluded roots: {}", EXCLUDED_ROOTS.join(", "))?;
        let top = self.top_unsupported();
        if !top.is_empty() {
            writeln!(f, "top unsupported extensions by bytes:")?;
            for (extension, count) in top {
                writeln!(
                    f,
                    "  {extension}  {} files  {} bytes",
                    count.files, count.bytes
                )?;
            }
        }
        Ok(())
    }
}
