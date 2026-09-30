//! The incremental relink: reach the cold build of a graph from a previous
//! view by re-resolving only the edges a file change can affect.
//!
//! An edge's outcome depends only on the nodes and import bindings behind the
//! lookup keys it consulted, so an edge none of whose keys a change touched
//! resolves as it did before. Relink re-resolves:
//!
//! 1. every edge of a changed or added file, whose entry is fresh from `next`;
//! 2. every edge of an unchanged file whose recorded keys intersect the
//!    [`touched_keys`] of the old and new entries of every changed, added or
//!    removed file;
//! 3. every edge of an unchanged file bound to, or listing as a candidate, a
//!    node of a changed or removed file, whether or not a key matched.
//!
//! A bind or candidate set extraction made is final and never selected.

use std::collections::BTreeSet;

use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::resolve::{eligible, resolution_stats, resolve_edges, touched_keys, EdgeRef};
use crate::context::source_graph::{SourceEdge, SourceNodeKind};

use super::build::{build_cold, count_resolution_run};
use super::{ResolvedView, ViewIdentity, ViewOrigin};

/// Resolve `next`, which holds extraction-time edges, starting from
/// `previous`. Falls back to [`build_cold`] unless `previous` was produced by
/// the same schema, extractors and resolver as `identity` names.
pub fn relink(
    previous: &ResolvedView,
    next: ResolvedGraph,
    identity: ViewIdentity,
) -> ResolvedView {
    if !identity.relinkable_from(&previous.identity) {
        return build_cold(next, identity);
    }
    count_resolution_run();
    let diff = FileDiff::between(&previous.graph, &next);
    let reopened = reopened_edges(previous, &next, &diff);
    let mut selected = fresh_edges(&next, &diff);
    selected.extend(reopened.iter().cloned());
    let mut graph = assemble(&previous.graph, next, &diff.unchanged, &reopened);
    let keys = resolve_edges(&mut graph, &selected);

    let mut deps = previous.deps.clone();
    deps.forget(|edge| diff.is_stale(&edge.path) || reopened.contains(edge));
    deps.record(keys);
    let stats = resolution_stats(&graph);
    ResolvedView {
        identity,
        graph,
        stats,
        deps,
        origin: ViewOrigin::Built,
    }
}

/// How the files of `next` relate to those of the previous graph, by
/// `content_hash`. An entry with an empty hash, an unreadable file, is never
/// unchanged: its coverage detail can differ between two equal hashes.
#[derive(Default)]
struct FileDiff {
    unchanged: BTreeSet<String>,
    changed: BTreeSet<String>,
    added: BTreeSet<String>,
    removed: BTreeSet<String>,
}

impl FileDiff {
    fn between(previous: &ResolvedGraph, next: &ResolvedGraph) -> Self {
        let mut diff = Self::default();
        for (path, entry) in &next.files {
            let bucket = match previous.files.get(path) {
                Some(old)
                    if !entry.content_hash.is_empty() && old.content_hash == entry.content_hash =>
                {
                    &mut diff.unchanged
                }
                Some(_) => &mut diff.changed,
                None => &mut diff.added,
            };
            bucket.insert(path.clone());
        }
        diff.removed = previous
            .files
            .keys()
            .filter(|path| !next.files.contains_key(*path))
            .cloned()
            .collect();
        diff
    }

    /// Whether the previous entry of `path` no longer describes the file.
    fn is_stale(&self, path: &str) -> bool {
        self.changed.contains(path) || self.removed.contains(path)
    }

    /// Every key the change can invalidate: the old and new entries of a
    /// changed file, the new entry of an added file and the old entry of a
    /// removed one. The `pathset:` keys come with an added or removed file,
    /// and with a changed file that gains or loses its file node (an
    /// unreadable entry has none), which enters or leaves the path index.
    fn touched(&self, previous: &ResolvedGraph, next: &ResolvedGraph) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for path in &self.changed {
            let (old, new) = (previous.files.get(path), next.files.get(path));
            let path_set_changed = has_file_node(old) != has_file_node(new);
            for entry in old.into_iter().chain(new) {
                keys.extend(touched_keys(entry, path_set_changed));
            }
        }
        for (paths, graph) in [(&self.added, next), (&self.removed, previous)] {
            for entry in paths.iter().filter_map(|path| graph.files.get(path)) {
                keys.extend(touched_keys(entry, true));
            }
        }
        keys
    }
}

/// Whether `entry` holds the file node the path index is built from.
fn has_file_node(entry: Option<&FileEntry>) -> bool {
    entry.is_some_and(|entry| {
        entry
            .nodes
            .iter()
            .any(|node| node.kind == SourceNodeKind::File)
    })
}

/// The edges of unchanged files to unbind and re-resolve (rules 2 and 3 of
/// the module docs), less every edge extraction decided.
fn reopened_edges(
    previous: &ResolvedView,
    next: &ResolvedGraph,
    diff: &FileDiff,
) -> BTreeSet<EdgeRef> {
    let touched = diff.touched(&previous.graph, next);
    let mut reopened: BTreeSet<EdgeRef> = touched
        .iter()
        .flat_map(|key| previous.deps.edges(key))
        .filter(|edge| diff.unchanged.contains(&edge.path))
        .cloned()
        .collect();
    reopened.extend(edges_into_stale_files(&previous.graph, diff));
    reopened.retain(|edge| left_open_by_extraction(next, edge));
    reopened
}

/// Edges of unchanged files bound to, or listing as a candidate, a node of a
/// changed or removed file.
fn edges_into_stale_files(previous: &ResolvedGraph, diff: &FileDiff) -> Vec<EdgeRef> {
    let stale_nodes: BTreeSet<&str> = diff
        .changed
        .iter()
        .chain(&diff.removed)
        .filter_map(|path| previous.files.get(path))
        .flat_map(|entry| entry.nodes.iter().map(|node| node.id.as_str()))
        .collect();
    let names_stale = |edge: &SourceEdge| {
        stale_nodes.contains(edge.to.as_str())
            || edge
                .candidates
                .iter()
                .any(|id| stale_nodes.contains(id.as_str()))
    };
    let mut edges = Vec::new();
    for path in &diff.unchanged {
        let Some(entry) = previous.files.get(path) else {
            continue;
        };
        for (index, edge) in entry.edges.iter().enumerate() {
            if names_stale(edge) {
                edges.push(EdgeRef {
                    path: path.clone(),
                    index,
                });
            }
        }
    }
    edges
}

/// Whether extraction left `edge` open for the resolver: its extraction-time
/// form is [`eligible`]. An unchanged file's entry in `next` holds its
/// extraction-time edges at the indices `previous` has them (the identity
/// gate guarantees one extractor produced both), so it tells a bind or a
/// candidate set extraction made, which is final, from one resolution made.
fn left_open_by_extraction(next: &ResolvedGraph, edge: &EdgeRef) -> bool {
    next.files
        .get(&edge.path)
        .and_then(|entry| entry.edges.get(edge.index))
        .is_some_and(eligible)
}

/// Every edge of a changed or added file.
fn fresh_edges(next: &ResolvedGraph, diff: &FileDiff) -> BTreeSet<EdgeRef> {
    diff.changed
        .iter()
        .chain(&diff.added)
        .filter_map(|path| Some((path, next.files.get(path)?)))
        .flat_map(|(path, entry)| {
            (0..entry.edges.len()).map(move |index| EdgeRef {
                path: path.clone(),
                index,
            })
        })
        .collect()
}

/// `next` with every unchanged file's resolved entry copied from `previous`
/// and each reopened edge unbound. The header stays `next`'s.
fn assemble(
    previous: &ResolvedGraph,
    mut next: ResolvedGraph,
    unchanged: &BTreeSet<String>,
    reopened: &BTreeSet<EdgeRef>,
) -> ResolvedGraph {
    for path in unchanged {
        if let (Some(slot), Some(resolved)) = (next.files.get_mut(path), previous.files.get(path)) {
            *slot = resolved.clone();
        }
    }
    for edge in reopened {
        let entry = next.files.get_mut(&edge.path);
        if let Some(reopened_edge) = entry.and_then(|entry| entry.edges.get_mut(edge.index)) {
            reopened_edge.unbind();
        }
    }
    next
}
