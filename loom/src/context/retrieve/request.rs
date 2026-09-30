//! Request construction for [`super::retrieve_for_stage`]: the id check that runs
//! before ranking and the packer's request built after it.

use super::{reject_unknown_require_ids, StageQuery};
use crate::context::graph_store::ResolvedGraph;
use crate::context::pack::PackRequest;
use crate::context::rank_source::intent::{classify, QueryIntent};
use crate::context::schema::{Channel, TextSearchHint};
use crate::context::store::StoreState;
use crate::fs::knowledge::catalog::Catalog;
use anyhow::Result;

/// Reject a `--require-id` this query's scope cannot possibly hold.
///
/// `graph` is offered to the check only when the source channel is actually in
/// scope: see [`reject_unknown_require_ids`]'s doc comment for why accepting a
/// source-node id for a query that will never rank source nodes reintroduces
/// the exact silent no-op that function exists to prevent.
pub(super) fn check_require_ids(
    query: &StageQuery,
    catalog: &Catalog,
    graph: Option<&ResolvedGraph>,
) -> Result<()> {
    let source_graph = query
        .scope
        .contains(&Channel::Source)
        .then_some(graph)
        .flatten();
    reject_unknown_require_ids(catalog, source_graph, &query.required_ids)
}

pub(super) fn build_pack_request(
    query: &StageQuery,
    budget_tokens: usize,
    state: StoreState,
    dropped_terms: Vec<String>,
    surviving_terms: Vec<String>,
    degraded: Option<String>,
) -> PackRequest {
    PackRequest {
        query: query.text.clone(),
        scope: query.scope.clone(),
        budget_tokens,
        structural_freshness: state.structural,
        semantic_freshness: state.semantic,
        dropped_terms,
        surviving_terms,
        required_representation: query.required_representation,
        degraded,
        text_search: literal_search_hint(&query.text),
    }
}

/// The text-search hint for a literal query: the graph indexes no bodies, so
/// quoted text is answered by searching for it.
fn literal_search_hint(query_text: &str) -> Option<TextSearchHint> {
    match classify(query_text) {
        QueryIntent::Literal { text } => Some(TextSearchHint::for_pattern(&text)),
        _ => None,
    }
}
