//! The in-memory graph of a labelled corpus. Nothing is written anywhere.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

use super::labels::{Labels, LABELS_FILE, STORED_SUFFIX};
use crate::context::extract::dialect::dialect_for_path;
use crate::context::extract::{extract_file, registry};
use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::refresh::excluded;
use crate::context::view::{build_cold, ViewIdentity};

/// Extract every file of `dir` that has a dialect, then resolve the graph.
///
/// The walk ignores git. `labels.yaml` and `EXCLUDED_ROOTS` are skipped, and a
/// listed syntax-error file stored as `<path>.txt` loads as `<path>`.
pub(super) fn build_graph(dir: &Path, labels: &Labels) -> Result<ResolvedGraph> {
    let extractors = registry();
    let syntax_errors: BTreeSet<&str> = labels
        .syntax_error_files
        .iter()
        .map(String::as_str)
        .collect();
    let mut files = BTreeMap::new();
    for stored in walk(dir)? {
        if stored == LABELS_FILE || excluded(&stored) {
            continue;
        }
        let logical = logical_path(&stored, &syntax_errors);
        if dialect_for_path(Path::new(logical)).is_none() {
            continue;
        }
        let bytes = fs::read(dir.join(&stored)).with_context(|| format!("read {stored}"))?;
        let extraction = extract_file(&extractors, Path::new(logical), &bytes);
        files.insert(
            logical.to_string(),
            FileEntry::from_extraction(&bytes, extraction),
        );
    }
    for listed in &syntax_errors {
        if !files.contains_key(*listed) {
            bail!(
                "syntax_error_files names {listed}, but the corpus has no {listed}{STORED_SUFFIX}"
            );
        }
    }
    let graph = ResolvedGraph {
        files,
        ..ResolvedGraph::default()
    };
    Ok(build_cold(graph, ViewIdentity::current("", "")).graph)
}

/// The path a stored file loads under: a listed syntax-error file loses its
/// `.txt` suffix, every other file keeps its name.
fn logical_path<'a>(stored: &'a str, syntax_errors: &BTreeSet<&str>) -> &'a str {
    match stored.strip_suffix(STORED_SUFFIX) {
        Some(real) if syntax_errors.contains(real) => real,
        _ => stored,
    }
}

/// Every regular file under `dir` as a forward-slashed path relative to it, in
/// sorted order. Symbolic links are not followed.
fn walk(dir: &Path) -> Result<Vec<String>> {
    let mut found = Vec::new();
    walk_into(dir, "", &mut found)?;
    found.sort();
    Ok(found)
}

fn walk_into(dir: &Path, prefix: &str, found: &mut Vec<String>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("read {}", dir.display()))?;
        let relative = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if excluded(&relative) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk_into(&entry.path(), &format!("{relative}/"), found)?;
        } else if kind.is_file() {
            found.push(relative);
        }
    }
    Ok(())
}
