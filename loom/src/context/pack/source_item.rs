//! Building a [`ContextItem`] from a ranked source-graph node.

use super::excerpt::bounded_excerpt;
use super::finalize_item;
use crate::context::freshness::GraphState;
use crate::context::rank::{neighbor_explanation, RankedCandidate};
use crate::context::render::rendered_item_tokens;
use crate::context::schema::{
    Channel, ChunkId, ContextItem, FileCoverage, ItemKind, LifecycleState, RequiredRepresentation,
    SourceNode, SourcePointer,
};

/// The caveat on a node extracted from a file that was only partly parsed.
const PARTIAL_COVERAGE_CAVEAT: &str = "partial coverage";

/// Add the snapshot caveat to a source item when the graph it came from is not
/// current, joining any caveat it already carries (`partial coverage; snapshot
/// stale`), and re-price the item: the caveat is rendered, so it costs tokens.
/// Knowledge items are untouched; they are not read from the snapshot.
pub(super) fn stamp_snapshot_caveat(item: &mut ContextItem, state: GraphState) {
    if item.kind != ItemKind::SourceNode || state == GraphState::Current {
        return;
    }
    let snapshot = format!("snapshot {}", state.as_str());
    item.caveat = Some(match item.caveat.take() {
        Some(existing) => format!("{existing}; {snapshot}"),
        None => snapshot,
    });
    item.token_count = rendered_item_tokens(item);
}

/// Build one `ContextItem` from a ranked candidate and its backing source node.
///
/// `state` is always [`LifecycleState::Active`]: a source node has no curation
/// lifecycle (draft/deprecated/superseded) the way a hand-written knowledge
/// chunk does — it is simply whatever the code on disk currently says. Do not
/// try to derive one from `node.coverage`; that describes extraction quality,
/// not trustworthiness.
///
/// `content_hash` is `node.body_hash`, already `sha256:<hex>` over this node's
/// exact source bytes — strictly more precise than the owning file's hash for
/// the delivery-record suppression `ContextItem::content_hash` feeds, since it
/// changes only when this node's own bytes do.
///
/// Under [`RequiredRepresentation::Compact`], `excerpt` goes through
/// [`bounded_excerpt`], never `crate::utils::truncate_for_display`:
/// `bounded_excerpt` is what enforces the documented contract on
/// `ContextItem::excerpt` (bounded by `schema::EXCERPT_MAX_TOKENS`, truncated
/// text ends with the schema truncation marker on its own line). A signature
/// is short, so this is nearly always a no-op, but using the other helper
/// would silently make source items the only ones in the corpus violating
/// that contract.
///
/// Under [`RequiredRepresentation::Full`] — the default, and what every
/// `--require-id` reservation gets unless the caller asks for `Compact` —
/// `excerpt` is `node.signature.clone()` verbatim, with no bound at all:
/// `Full` exists precisely to let a caller demand the whole unit regardless
/// of `EXCERPT_MAX_TOKENS`, so the bound above does not apply to it.
///
/// No file reads here or anywhere else in the packer: retrieval is a pure
/// function of bytes already loaded into the `SourceNode`, not of the working
/// tree at query time.
pub(super) fn build_source_item(
    candidate: &RankedCandidate,
    node: &SourceNode,
    terms: &[String],
    representation: RequiredRepresentation,
) -> ContextItem {
    let (excerpt, truncated) = match representation {
        RequiredRepresentation::Full => (node.signature.clone(), false),
        RequiredRepresentation::Compact => bounded_excerpt(&node.signature, terms),
    };
    finalize_item(ContextItem {
        id: ChunkId::from(node.id.as_str()),
        kind: ItemKind::SourceNode,
        pointer: SourcePointer {
            path: node.path.clone(),
            anchor: String::new(),
            line_start: Some(node.span.line_start),
            line_end: Some(node.span.line_end),
        },
        summary: format!(
            "{} {} - {}:{}-{}",
            node.kind.as_str(),
            node.scope.join("::"),
            node.path.display(),
            node.span.line_start,
            node.span.line_end
        ),
        source: Channel::Source,
        token_count: 0,
        score: candidate.score,
        reasons: candidate.reasons.clone(),
        // See `build_chunk_item`: the cap rides on the candidate, not the
        // reasons, so both item builders must ask the candidate.
        confidence: candidate.confidence(),
        state: LifecycleState::Active,
        content_hash: node.body_hash.clone(),
        excerpt: Some(excerpt),
        truncated,
        matched_term_count: candidate.matched_term_count,
        explanation: candidate.via.as_ref().map(neighbor_explanation),
        caveat: (!matches!(node.coverage, FileCoverage::Full))
            .then(|| PARTIAL_COVERAGE_CAVEAT.to_string()),
        window: None,
    })
}
