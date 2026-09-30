//! Relationship routing: seed the node a query names and admit its direct
//! neighbours in the asked direction.

use super::{find_mut, scored_node, source_candidate, unambiguous_nodes, RouteInputs};
use crate::context::config::RetrievalConfig;
use crate::context::graph_store::ResolvedGraph;
use crate::context::rank::{EdgeDirection, NeighborVia, RankedCandidate, BOOST_EXACT_SYMBOL};
use crate::context::rank_source::expand::{
    is_expandable, max_neighbor_score, neighbour_tokens, ExpansionSpend,
    MAX_EXAMINED_NEIGHBORS_PER_SEED, MAX_EXPANDED, MAX_EXPANDED_TOKENS,
    MIN_NEIGHBOR_EDGE_CONFIDENCE, NEIGHBOR_SCORE_FACTOR,
};
use crate::context::rank_source::intent::RelationDirection;
use crate::context::rank_source::paths::apply_test_path_factor;
use crate::context::rank_source::ScoredNode;
use crate::context::resolve::{
    direct_callees, direct_callers, direct_references, impact_with, ImpactOptions, Neighbor,
};
use crate::context::schema::{
    Confidence, EdgeProvenance, FileCoverage, SelectionReason, SourceEdgeKind, SourceNode,
};
use std::collections::{BTreeMap, BTreeSet};

/// A seed with the score its neighbours derive from and the edges to them.
type Seed<'a> = (&'a SourceNode, f32, Vec<RelationEdge>);

/// One edge from a relationship seed to a neighbour, in the shape
/// [`NeighborVia`] records.
struct RelationEdge {
    neighbour: String,
    kind: SourceEdgeKind,
    provenance: EdgeProvenance,
    confidence: f32,
    site_line: Option<usize>,
}

impl From<Neighbor> for RelationEdge {
    fn from(row: Neighbor) -> Self {
        Self {
            site_line: row
                .sites
                .first()
                .map(|site| site.line_start)
                .filter(|line| *line > 0),
            neighbour: row.id,
            kind: row.edge_kind,
            provenance: row.provenance,
            confidence: row.confidence,
        }
    }
}

/// Seed every fully extracted node `symbol` names as an exact symbol, then
/// admit the seeds' direct neighbours in `direction` as graph neighbours. A
/// name too many nodes share seeds nothing. Returns the count and the estimated
/// rendered tokens of the neighbours admitted.
///
/// Neighbours stop at [`MAX_EXPANDED`] in count and [`MAX_EXPANDED_TOKENS`] in
/// estimated tokens; expansion continues from the spend returned here, so one
/// budget covers the whole query. A file node, like a partly extracted one, is
/// never admitted as a neighbour (`is_expandable`).
///
/// Partially extracted seeds are skipped for the reason
/// `withhold_partial_coverage` gives: an exact rung claims high confidence, and
/// incomplete extraction cannot back that claim.
pub(super) fn admit_relationship<'a>(
    direction: RelationDirection,
    symbol: &str,
    scored: &mut Vec<ScoredNode<'a>>,
    inputs: &RouteInputs<'a, '_>,
) -> ExpansionSpend {
    let seeds = admit_seeds(direction, symbol, scored, inputs);
    let wanted: BTreeSet<&str> = seeds
        .iter()
        .flat_map(|(_, _, edges)| edges.iter().map(|edge| edge.neighbour.as_str()))
        .collect();
    let by_id = index_nodes(inputs.nodes, &wanted);
    let mut spend = ExpansionSpend::default();
    for (seed, seed_score, edges) in seeds {
        for edge in edges {
            if spend.neighbours == MAX_EXPANDED {
                return spend;
            }
            let Some(node) = by_id.get(edge.neighbour.as_str()).copied() else {
                continue;
            };
            if node.id == seed.id || !is_expandable(node) {
                continue;
            }
            let via = NeighborVia {
                seed: seed.id.clone(),
                edge_kind: edge.kind,
                direction: edge_direction(direction),
                provenance: edge.provenance,
                site_line: edge.site_line,
            };
            let cost = neighbour_tokens(node, &via);
            // A neighbour already ranked only merges its edge: it adds no
            // rendered item, so it costs nothing.
            let is_new = find_mut(scored, &node.id).is_none();
            if is_new && spend.tokens + cost > MAX_EXPANDED_TOKENS {
                return spend;
            }
            if admit_neighbour(node, via, seed_score, scored, inputs.config) {
                spend.neighbours += 1;
                spend.tokens += cost;
            }
        }
    }
    spend
}

/// Admit every fully extracted node `symbol` names as a seed, with the edges
/// to its neighbours in `direction`. A symbol too many nodes share seeds
/// nothing ([`unambiguous_nodes`]): an exact-symbol seed claims the node was
/// meant, which a name like `new` cannot back.
fn admit_seeds<'a>(
    direction: RelationDirection,
    symbol: &str,
    scored: &mut Vec<ScoredNode<'a>>,
    inputs: &RouteInputs<'a, '_>,
) -> Vec<Seed<'a>> {
    unambiguous_nodes(symbol, inputs.nodes)
        .into_iter()
        .filter(|(_, node)| matches!(node.coverage, FileCoverage::Full))
        .map(|(index, seed)| {
            let seed_score = admit_seed(index, seed, scored, inputs);
            let edges = relation_edges(inputs.graph, &seed.id, direction);
            (seed, seed_score, edges)
        })
        .collect()
}

/// Admit `seed` as an exact-symbol candidate unless it is one already, and
/// return the score its neighbours are derived from.
fn admit_seed<'a>(
    index: usize,
    seed: &'a SourceNode,
    scored: &mut Vec<ScoredNode<'a>>,
    inputs: &RouteInputs<'a, '_>,
) -> f32 {
    if let Some(existing) = find_mut(scored, &seed.id) {
        return existing.candidate.score;
    }
    let (lexical, matched_term_count) = inputs.corpus.score(inputs.nodes.len() as f32, index);
    let mut candidate = source_candidate(seed, 0.0, SelectionReason::ExactSymbol);
    // As in `score_node`: once a rung fired, any lexical score rides along.
    let mut score = BOOST_EXACT_SYMBOL;
    if matched_term_count > 0 {
        candidate.reasons.push(SelectionReason::Lexical);
        score += lexical;
    }
    candidate.score = apply_test_path_factor(seed, score, inputs.config);
    candidate.matched_term_count = matched_term_count;
    let seed_score = candidate.score;
    scored.push(scored_node(seed, candidate));
    seed_score
}

/// Admit `node` as a graph neighbour, or merge the reason and the edge into
/// the candidate it already is. True when a new candidate was added.
fn admit_neighbour<'a>(
    node: &'a SourceNode,
    via: NeighborVia,
    seed_score: f32,
    scored: &mut Vec<ScoredNode<'a>>,
    config: &RetrievalConfig,
) -> bool {
    if let Some(existing) = find_mut(scored, &node.id) {
        let candidate = &mut existing.candidate;
        if !candidate.reasons.contains(&SelectionReason::GraphNeighbor) {
            candidate.reasons.push(SelectionReason::GraphNeighbor);
        }
        if candidate.via.is_none() {
            candidate.via = Some(via);
        }
        return false;
    }
    // The ceiling `expand` puts on its neighbours: never above the weakest
    // score a genuine exact-symbol hit can have.
    let score = apply_test_path_factor(
        node,
        (seed_score * NEIGHBOR_SCORE_FACTOR).min(max_neighbor_score(config)),
        config,
    );
    let candidate = RankedCandidate {
        confidence_ceiling: Some(Confidence::Medium),
        via: Some(via),
        ..source_candidate(node, score, SelectionReason::GraphNeighbor)
    };
    scored.push(scored_node(node, candidate));
    true
}

/// The seed's neighbours in `direction` over edges of at least
/// [`MIN_NEIGHBOR_EDGE_CONFIDENCE`], strongest first.
fn relation_edges(
    graph: &ResolvedGraph,
    seed_id: &str,
    direction: RelationDirection,
) -> Vec<RelationEdge> {
    let limit = MAX_EXAMINED_NEIGHBORS_PER_SEED;
    let rows = match direction {
        RelationDirection::Callers => direct_callers(graph, seed_id, limit).0,
        RelationDirection::Callees => direct_callees(graph, seed_id, limit).0,
        RelationDirection::References => direct_references(graph, seed_id, limit).0,
        RelationDirection::Impact => return impact_edges(graph, seed_id),
    };
    rows.into_iter()
        .map(RelationEdge::from)
        .filter(|edge| edge.confidence >= MIN_NEIGHBOR_EDGE_CONFIDENCE)
        .collect()
}

/// Direct dependents of the seed through any semantic edge. An impact hit
/// records its edge but not the site, so these carry no site line.
fn impact_edges(graph: &ResolvedGraph, seed_id: &str) -> Vec<RelationEdge> {
    let options = ImpactOptions {
        max_depth: 1,
        limit: MAX_EXAMINED_NEIGHBORS_PER_SEED,
        min_confidence: MIN_NEIGHBOR_EDGE_CONFIDENCE,
        ..ImpactOptions::default()
    };
    impact_with(graph, seed_id, &options)
        .hits
        .into_iter()
        .map(|hit| RelationEdge {
            neighbour: hit.id,
            kind: hit.weakest_kind,
            provenance: hit.weakest_provenance,
            confidence: hit.min_confidence,
            site_line: None,
        })
        .collect()
}

/// The edge's direction relative to the neighbour: every direction but
/// `Callees` finds a neighbour that points at the seed.
fn edge_direction(direction: RelationDirection) -> EdgeDirection {
    match direction {
        RelationDirection::Callees => EdgeDirection::Incoming,
        RelationDirection::Callers | RelationDirection::References | RelationDirection::Impact => {
            EdgeDirection::Outgoing
        }
    }
}

/// Index just the nodes in `wanted`, so a relationship query does not key
/// every node in the graph to look up a handful of neighbours.
fn index_nodes<'a>(
    nodes: &[&'a SourceNode],
    wanted: &BTreeSet<&str>,
) -> BTreeMap<&'a str, &'a SourceNode> {
    nodes
        .iter()
        .copied()
        .filter(|node| wanted.contains(node.id.as_str()))
        .map(|node| (node.id.as_str(), node))
        .collect()
}
