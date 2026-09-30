//! How a view argument selects graph nodes, shared by every view.

use std::path::Path;

use colored::Colorize;

use crate::context::graph_store::ResolvedGraph;
use crate::context::resolve::node_names;
use crate::context::source_graph::SourceNode;

use super::{project_relative, IMPACT_MAX_STARTS};

/// How an argument selected its nodes; the label every view carries so a
/// fuzzy answer never reads as an exact one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// The argument was an exact node id or a file id. Never falls back.
    Id,
    /// The argument equals a node name, case-sensitively.
    Exact,
    /// No exact name exists; the case-insensitive substring fallback ran.
    Substring,
}

impl MatchMode {
    /// The `"match"` value of a JSON view object.
    pub fn as_str(self) -> &'static str {
        match self {
            MatchMode::Id => "id",
            MatchMode::Exact => "exact",
            MatchMode::Substring => "substring",
        }
    }

    /// Text appended to a human heading; empty unless the fallback ran.
    pub fn label(self) -> &'static str {
        match self {
            MatchMode::Substring => " (substring matches)",
            MatchMode::Id | MatchMode::Exact => "",
        }
    }
}

/// Select nodes for `arg`: an argument containing `#` is an exact node id;
/// otherwise an exact case-sensitive name, falling back to a case-insensitive
/// substring only when no node carries that exact name.
pub(super) fn find_symbol_matches<'a>(
    graph: &'a ResolvedGraph,
    arg: &str,
) -> (Vec<&'a SourceNode>, MatchMode) {
    if arg.contains('#') {
        return (graph.node(arg).into_iter().collect(), MatchMode::Id);
    }
    let exact: Vec<&SourceNode> = graph
        .nodes()
        .filter(|node| node_names(node).iter().any(|name| name == arg))
        .collect();
    if !exact.is_empty() {
        return (exact, MatchMode::Exact);
    }
    let needle = arg.to_lowercase();
    let substring = graph
        .nodes()
        .filter(|node| {
            node_names(node)
                .iter()
                .any(|name| name.to_lowercase().contains(&needle))
        })
        .collect();
    (substring, MatchMode::Substring)
}

/// The start nodes of a callers, callees, references or impact query.
pub(super) struct ResolvedStarts {
    pub file_start: Option<String>,
    pub symbol_matches: Vec<String>,
    pub starts: Vec<String>,
    pub mode: MatchMode,
}

impl ResolvedStarts {
    /// The starts a view expands, capped at [`IMPACT_MAX_STARTS`].
    pub fn shown(&self) -> &[String] {
        &self.starts[..self.starts.len().min(IMPACT_MAX_STARTS)]
    }

    /// Ambiguous starts beyond the cap.
    pub fn suppressed(&self) -> usize {
        self.starts.len() - self.shown().len()
    }
}

/// A path naming an indexed file selects that file; otherwise the symbol
/// matches are the starts.
pub(super) fn resolve_starts(
    graph: &ResolvedGraph,
    project_root: &Path,
    arg: &str,
) -> ResolvedStarts {
    let (nodes, mode) = find_symbol_matches(graph, arg);
    let symbol_matches: Vec<String> = nodes.into_iter().map(|node| node.id.clone()).collect();
    let file_start = if mode == MatchMode::Id {
        None
    } else {
        project_relative(project_root, arg).filter(|rel| graph.files.contains_key(rel))
    };
    match &file_start {
        Some(rel) => ResolvedStarts {
            starts: vec![rel.clone()],
            mode: MatchMode::Id,
            file_start,
            symbol_matches,
        },
        None => ResolvedStarts {
            starts: symbol_matches.clone(),
            mode,
            file_start,
            symbol_matches,
        },
    }
}

pub(super) fn match_note(resolved: &ResolvedStarts, safe_arg: &str) -> Option<String> {
    if resolved.file_start.is_some() && !resolved.symbol_matches.is_empty() {
        Some(format!(
            "note: {safe_arg} also matches {} symbol definition(s); showing the file's impact only",
            resolved.symbol_matches.len()
        ))
    } else if resolved.starts.len() > 1 {
        Some(format!(
            "{} {} definitions match {safe_arg}; showing impact for each",
            "→".cyan().bold(),
            resolved.starts.len()
        ))
    } else {
        None
    }
}
