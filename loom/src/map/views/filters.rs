//! `--path` and `--lang` filters, with the counts every view reports.

use serde_json::{json, Value};

use crate::context::extract::dialect::DIALECTS;
use crate::context::graph_store::ResolvedGraph;
use crate::context::source_graph::EdgeProvenance;

const PROVENANCES: [EdgeProvenance; 7] = [
    EdgeProvenance::Structural,
    EdgeProvenance::Compiler,
    EdgeProvenance::Receiver,
    EdgeProvenance::Import,
    EdgeProvenance::LocalName,
    EdgeProvenance::UniqueName,
    EdgeProvenance::Syntax,
];

/// The path prefix and dialect a view keeps; both optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewFilters {
    path: Option<String>,
    lang: Option<String>,
}

/// Hits a filter dropped, per filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Removed {
    pub path: usize,
    pub lang: usize,
}

impl ViewFilters {
    /// `path` is normalized to forward slashes without a leading `./` or a
    /// trailing `/`; an empty prefix, or a bare `.` (the project root), filters
    /// nothing.
    pub fn new(path: Option<String>, lang: Option<String>) -> Self {
        let path = path
            .map(|raw| {
                let slashed = raw.replace('\\', "/");
                let trimmed = slashed.strip_prefix("./").unwrap_or(&slashed);
                trimmed.trim_end_matches('/').to_string()
            })
            .filter(|prefix| !prefix.is_empty() && prefix != ".");
        Self {
            path,
            lang: lang.map(|name| name.to_ascii_lowercase()),
        }
    }

    /// The normalized prefix, for callers that filter on their own first.
    pub(super) fn path_prefix(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Whether a hit in `path` written in `language` stays. A rejection is
    /// counted in `removed` against the first filter that refused it.
    pub(super) fn admit(&self, path: &str, language: &str, removed: &mut Removed) -> bool {
        if let Some(prefix) = &self.path {
            if !path_within(prefix, path) {
                removed.path += 1;
                return false;
            }
        }
        if let Some(lang) = &self.lang {
            if lang != language {
                removed.lang += 1;
                return false;
            }
        }
        true
    }

    /// The `"filters"` object of a view: `applied` is `"scan"` when the filter
    /// ran while collecting and `"display"` when it ran over finished hits.
    pub(super) fn to_json(&self, removed: Removed, applied: &str) -> Value {
        json!({
            "path": {"value": self.path, "applied": applied, "filtered_out": removed.path},
            "lang": {"value": self.lang, "applied": applied, "filtered_out": removed.lang},
        })
    }

    /// One human line naming each active filter and what it removed.
    pub(super) fn note(&self, removed: Removed) -> Option<String> {
        let mut stages = Vec::new();
        if let Some(prefix) = &self.path {
            stages.push(format!("path {prefix} removed {}", removed.path));
        }
        if let Some(lang) = &self.lang {
            stages.push(format!("lang {lang} removed {}", removed.lang));
        }
        (!stages.is_empty()).then(|| format!("  filters: {}", stages.join("; ")))
    }
}

/// Component-aware prefix test: `src/a` holds `src/a` and `src/a/x.rs`, never
/// `src/ab.rs`.
fn path_within(prefix: &str, path: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The dialect a graph file was written in, or `""` for an unindexed path.
pub(super) fn language_of<'a>(graph: &'a ResolvedGraph, path: &str) -> &'a str {
    graph
        .files
        .get(path)
        .and_then(|entry| entry.nodes.first())
        .map_or("", |node| node.language.as_str())
}

/// Keep the first `limit` rows (zero keeps all) and return how many were cut.
pub(super) fn cap<T>(rows: &mut Vec<T>, limit: usize) -> usize {
    if limit == 0 || rows.len() <= limit {
        return 0;
    }
    let suppressed = rows.len() - limit;
    rows.truncate(limit);
    suppressed
}

/// clap value parser for `--lang`: a dialect id of the registry table.
pub fn parse_language(value: &str) -> Result<String, String> {
    let lowered = value.to_ascii_lowercase();
    if DIALECTS.iter().any(|dialect| dialect.id == lowered) {
        return Ok(lowered);
    }
    let valid: Vec<&str> = DIALECTS.iter().map(|dialect| dialect.id).collect();
    Err(format!(
        "unknown language '{value}'; valid languages: {}",
        valid.join(", ")
    ))
}

/// clap value parser for `--evidence`: an edge provenance name.
pub fn parse_provenance(value: &str) -> Result<EdgeProvenance, String> {
    PROVENANCES
        .into_iter()
        .find(|provenance| provenance.as_str() == value)
        .ok_or_else(|| {
            let valid: Vec<&str> = PROVENANCES.iter().map(EdgeProvenance::as_str).collect();
            format!(
                "unknown evidence '{value}'; valid evidence: {}",
                valid.join(", ")
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_dot_is_the_project_root_and_filters_nothing() {
        for raw in [".", "./", "./.", "", "/"] {
            let filters = ViewFilters::new(Some(raw.to_string()), None);
            let mut removed = Removed::default();

            assert_eq!(filters.path_prefix(), None, "{raw:?}");
            assert!(filters.admit("src/a.rs", "rust", &mut removed), "{raw:?}");
            assert_eq!(removed, Removed::default(), "{raw:?}");
        }
    }

    #[test]
    fn a_dotted_directory_name_is_still_a_prefix() {
        let filters = ViewFilters::new(Some("./.github/".to_string()), None);

        assert_eq!(filters.path_prefix(), Some(".github"));
    }
}
