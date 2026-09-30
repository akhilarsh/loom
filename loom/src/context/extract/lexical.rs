//! File-level fallback for anything no grammar claims.
//!
//! A file loom cannot parse must still be *findable*: retrieval ranks over
//! paths and lexical overlap as well as symbols, so dropping unparseable files
//! would make whole directories invisible. Every such file gets exactly one
//! node, tagged [`FileCoverage::LexicalOnly`] so a consumer can tell "no
//! symbols here" from "no symbols found here".

use std::path::Path;

use super::dialect::dialect_for_path;
use super::FileExtraction;
use crate::context::source_graph::{FileCoverage, NodeLanguage};

/// `parser_version` stamped on a node no grammar produced.
pub const LEXICAL_PARSER_VERSION: &str = "lexical+v1";

/// Language tag for a path, by extension.
///
/// Returns the dialect's [`NodeLanguage`] when the extension is recognized, and
/// `Other(<extension>)` otherwise so output can still name what the file was.
pub fn language_for_path(path: &Path) -> NodeLanguage {
    if let Some(dialect) = dialect_for_path(path) {
        return dialect.language.clone();
    }
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    NodeLanguage::Other(extension)
}

/// Build the file-level-only extraction for `path`.
pub fn extract(path: &Path, bytes: &[u8]) -> FileExtraction {
    let language = language_for_path(path);
    let detail = format!("no source-graph extractor for {language}");
    FileExtraction::file_level(
        path,
        bytes,
        language,
        LEXICAL_PARSER_VERSION.to_string(),
        FileCoverage::LexicalOnly { detail },
    )
}
