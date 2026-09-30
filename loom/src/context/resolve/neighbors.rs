//! Direct symbol neighbors for the callers, callees and references views.

use std::collections::BTreeMap;

use crate::context::graph_store::ResolvedGraph;
use crate::context::source_graph::{
    EdgeProvenance, SourceEdge, SourceEdgeKind, SourceNode, SourceNodeKind, Span,
    AMBIGUOUS_CANDIDATE_CONFIDENCE,
};

/// One symbol joined to the query node by one edge, with the sites that wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbor {
    pub id: String,
    pub kind: SourceNodeKind,
    pub path: String,
    pub edge_kind: SourceEdgeKind,
    pub confidence: f32,
    pub provenance: EdgeProvenance,
    /// The neighbour's declaration line.
    pub line_start: Option<usize>,
    /// The spelling written at the site.
    pub symbol: String,
    /// Call or reference sites, in the file named by `site_path`.
    pub sites: Vec<Span>,
    /// Path of the file holding `sites`: the file the edge was extracted from,
    /// which is the caller's file.
    pub site_path: String,
    /// `Some(n)` when this row is one member of an `n`-member candidate set.
    pub candidate_of: Option<usize>,
}

/// Symbols with a `Calls` edge into `node_id`, plus those whose ambiguous call
/// lists it as a candidate.
///
/// A call made at file top level has a file node as its origin and is not a
/// symbol, so it never appears here.
pub fn direct_callers(
    graph: &ResolvedGraph,
    node_id: &str,
    limit: usize,
) -> (Vec<Neighbor>, usize) {
    direct_neighbors(
        graph,
        node_id,
        limit,
        Direction::Incoming,
        SourceEdgeKind::Calls,
    )
}

/// Symbols a `Calls` edge out of `node_id` reaches, plus each member of an
/// ambiguous call's candidate set.
pub fn direct_callees(
    graph: &ResolvedGraph,
    node_id: &str,
    limit: usize,
) -> (Vec<Neighbor>, usize) {
    direct_neighbors(
        graph,
        node_id,
        limit,
        Direction::Outgoing,
        SourceEdgeKind::Calls,
    )
}

/// Symbols with a `References` edge into `node_id`, plus candidate members.
pub fn direct_references(
    graph: &ResolvedGraph,
    node_id: &str,
    limit: usize,
) -> (Vec<Neighbor>, usize) {
    direct_neighbors(
        graph,
        node_id,
        limit,
        Direction::Incoming,
        SourceEdgeKind::References,
    )
}

#[derive(Clone, Copy)]
enum Direction {
    Incoming,
    Outgoing,
}

fn direct_neighbors(
    graph: &ResolvedGraph,
    node_id: &str,
    limit: usize,
    direction: Direction,
    kind: SourceEdgeKind,
) -> (Vec<Neighbor>, usize) {
    let nodes = graph
        .nodes()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let Some(start) = nodes.get(node_id).copied().filter(|node| is_symbol(node)) else {
        return (Vec::new(), 0);
    };

    let mut neighbors = collect_neighbors(graph, &nodes, start, direction, kind);
    neighbors.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| first_line(a).cmp(&first_line(b)))
    });
    let suppressed = suppress_beyond_limit(&mut neighbors, limit);
    (neighbors, suppressed)
}

fn first_line(neighbor: &Neighbor) -> Option<usize> {
    neighbor.sites.first().map(|site| site.line_start)
}

fn collect_neighbors(
    graph: &ResolvedGraph,
    nodes: &BTreeMap<&str, &SourceNode>,
    start: &SourceNode,
    direction: Direction,
    kind: SourceEdgeKind,
) -> Vec<Neighbor> {
    let mut neighbors = Vec::new();
    // An edge lives in the entry of the file it was extracted from, so the map
    // key is where its sites are.
    for (site_path, entry) in &graph.files {
        for edge in entry.edges.iter().filter(|edge| edge.kind == kind) {
            for (endpoint_id, candidate_of) in endpoints(edge, &start.id, direction) {
                let Some(endpoint) = nodes.get(endpoint_id).copied().filter(|n| is_symbol(n))
                else {
                    continue;
                };
                neighbors.push(row(edge, endpoint, site_path, candidate_of));
            }
        }
    }
    neighbors
}

/// The far end of `edge` as seen from `start_id`: the bound target or origin,
/// and one entry per candidate the edge lists. The second field is the size of
/// the candidate set for a candidate entry.
fn endpoints<'a>(
    edge: &'a SourceEdge,
    start_id: &str,
    direction: Direction,
) -> Vec<(&'a str, Option<usize>)> {
    let set_size = Some(edge.candidates.len());
    let mut found = Vec::new();
    match direction {
        Direction::Incoming => {
            if !edge.is_unresolved() && edge.to == start_id {
                found.push((edge.from.as_str(), None));
            }
            if edge.candidates.iter().any(|id| id == start_id) {
                found.push((edge.from.as_str(), set_size));
            }
        }
        Direction::Outgoing if edge.from == start_id => {
            if !edge.is_unresolved() {
                found.push((edge.to.as_str(), None));
            }
            found.extend(edge.candidates.iter().map(|id| (id.as_str(), set_size)));
        }
        Direction::Outgoing => {}
    }
    found
}

fn row(
    edge: &SourceEdge,
    endpoint: &SourceNode,
    site_path: &str,
    candidate_of: Option<usize>,
) -> Neighbor {
    let (confidence, provenance) = match candidate_of {
        Some(_) => (AMBIGUOUS_CANDIDATE_CONFIDENCE, EdgeProvenance::Syntax),
        None => (edge.confidence, edge.provenance),
    };
    Neighbor {
        id: endpoint.id.clone(),
        kind: endpoint.kind,
        path: endpoint.path.to_string_lossy().into_owned(),
        edge_kind: edge.kind,
        confidence,
        provenance,
        line_start: Some(endpoint.span.line_start),
        symbol: edge.symbol.clone(),
        sites: edge.sites.clone(),
        site_path: site_path.to_string(),
        candidate_of,
    }
}

/// Drop everything past `limit`, reporting how many were dropped. `limit ==
/// 0` means unlimited.
fn suppress_beyond_limit(neighbors: &mut Vec<Neighbor>, limit: usize) -> usize {
    if limit > 0 && neighbors.len() > limit {
        let suppressed = neighbors.len() - limit;
        neighbors.truncate(limit);
        suppressed
    } else {
        0
    }
}

fn is_symbol(node: &SourceNode) -> bool {
    node.kind != SourceNodeKind::File
}

#[cfg(test)]
#[path = "tests_neighbors.rs"]
mod tests;
