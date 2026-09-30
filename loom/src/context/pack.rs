//! Budget-constrained construction of context packs.
//!
//! One selection rule is not purely budget-driven and lives next door in
//! `twins`: a tier-1 summary and the tier-2 topic it spilled restate each
//! other, so `select` spends the budget on the detail and keeps the summary
//! only as the fallback for when the detail does not fit.

pub(crate) mod excerpt;
mod required;
mod source_item;
pub(crate) mod twins;

use crate::context::freshness::GraphState;
use crate::context::graph_store::ResolvedGraph;
use crate::context::rank::RankedCandidate;
use crate::context::render::{literal_text_tokens, rendered_brief_tokens, rendered_item_tokens};
use crate::context::schema::{
    Channel, Confidence, ContextItem, ContextPack, Coverage, FileCoverage, Freshness, ItemKind,
    KnowledgeChunk, OmissionSummary, RequiredRepresentation, SourceNode, SourcePointer,
    TextSearchHint, UnmetRequirement,
};
use excerpt::bounded_excerpt;
use required::reserve_within_budget;
use source_item::{build_source_item, stamp_snapshot_caveat};
use twins::{details_before_summaries, explicitly_required, knowledge_twin};

use std::collections::{BTreeMap, BTreeSet};

/// Everything the packer needs besides the ranked list.
#[derive(Debug, Clone)]
pub struct PackRequest {
    /// Original query text.
    pub query: String,
    /// Retrieval channels included in this request.
    pub scope: Vec<Channel>,
    /// Maximum estimated token cost of returned items.
    pub budget_tokens: usize,
    /// Freshness of the structural retrieval layer.
    pub structural_freshness: Freshness,
    /// Freshness of the semantic retrieval layer.
    pub semantic_freshness: Freshness,
    /// Query terms the ranker dropped before scoring, copied onto the pack for
    /// observability. Retrieval takes these from the knowledge channel's
    /// corpus; see `retrieve::rank_channels`.
    pub dropped_terms: Vec<String>,
    /// Query terms that survived per-channel corpus stopwording.
    pub surviving_terms: Vec<String>,
    /// Representation reserved for explicitly required ids.
    pub required_representation: RequiredRepresentation,
    /// Why this pack was served from a knowingly incomplete index, when it was.
    ///
    /// Passed straight through to [`ContextPack::degraded`] so the wave that
    /// detects a missing base graph (A.11) only has to fill this field in
    /// `retrieve_for_stage`, with no further plumbing.
    pub degraded: Option<String>,
    /// The text search to suggest when the query asked for literal text; copied
    /// onto [`ContextPack::text_search`].
    pub text_search: Option<TextSearchHint>,
}

/// Multiplier on the score of a source candidate that carries a trust caveat:
/// once for a partly parsed file, once more for a snapshot that is not current.
const CAVEAT_SCORE_FACTOR: f32 = 0.6;

/// `ranked` with each caveated source candidate's score scaled by
/// [`CAVEAT_SCORE_FACTOR`], re-ordered so the walk in `select` sees the
/// demotion. The list is returned untouched when nothing is demoted.
///
/// Scores are not comparable across fusion tiers (`fuse`'s module doc), so the
/// re-sort keeps tier 1 (any exact rung) ahead of tier 2 and orders by score
/// only inside a tier. The sort is stable, so candidates whose score did not
/// change keep the order fusion gave them.
fn apply_caveat_factors(
    ranked: &[RankedCandidate],
    nodes: &BTreeMap<&str, &SourceNode>,
    state: GraphState,
) -> Vec<RankedCandidate> {
    let mut adjusted = ranked.to_vec();
    let mut demoted = false;
    for candidate in adjusted
        .iter_mut()
        .filter(|candidate| candidate.channel == Channel::Source)
    {
        let partial = nodes
            .get(candidate.id.as_str())
            .is_some_and(|node| !matches!(node.coverage, FileCoverage::Full));
        let caveats = i32::from(partial) + i32::from(state != GraphState::Current);
        if caveats > 0 {
            candidate.score *= CAVEAT_SCORE_FACTOR.powi(caveats);
            demoted = true;
        }
    }
    if demoted {
        // `from_reasons` is not `Low` exactly when an exact rung fired, which
        // is fusion's tier-1 test.
        let tier1 = |candidate: &RankedCandidate| {
            Confidence::from_reasons(&candidate.reasons) != Confidence::Low
        };
        adjusted.sort_by(|a, b| {
            tier1(b)
                .cmp(&tier1(a))
                .then_with(|| b.score.total_cmp(&a.score))
        });
    }
    adjusted
}

fn summary(chunk: &KnowledgeChunk) -> String {
    if !chunk.heading.is_empty() {
        return chunk.heading.clone();
    }
    let line = chunk
        .body
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .unwrap_or("");
    line.chars().take(120).collect()
}

/// Build one `ContextItem` from a ranked candidate and its backing chunk.
fn build_chunk_item(
    candidate: &RankedCandidate,
    chunk: &KnowledgeChunk,
    terms: &[String],
    representation: RequiredRepresentation,
) -> ContextItem {
    let (excerpt, truncated) = match representation {
        RequiredRepresentation::Full => (chunk.body.clone(), false),
        RequiredRepresentation::Compact => bounded_excerpt(&chunk.body, terms),
    };
    finalize_item(ContextItem {
        id: candidate.id.clone(),
        kind: ItemKind::KnowledgeChunk,
        pointer: SourcePointer {
            path: chunk.file.clone(),
            anchor: chunk.anchor.clone(),
            line_start: None,
            line_end: None,
        },
        summary: summary(chunk),
        source: candidate.channel,
        token_count: 0,
        score: candidate.score,
        reasons: candidate.reasons.clone(),
        // Never `Confidence::from_reasons` directly: the ranker can cap a
        // candidate below what its reasons imply (an exact match admitted only
        // by corpus rarity is `Medium`, not `High`), and that cap lives on the
        // candidate, not in the reason list.
        confidence: candidate.confidence(),
        state: chunk.state,
        content_hash: chunk.content_hash.clone(),
        excerpt: Some(excerpt),
        truncated,
        matched_term_count: candidate.matched_term_count,
        explanation: None,
        caveat: None,
        window: None,
    })
}

fn finalize_item(mut item: ContextItem) -> ContextItem {
    item.token_count = rendered_item_tokens(&item);
    item
}

/// Summarize coverage and omissions for a completed pack: how many of the
/// ranked candidates (and their tokens) made it into `items`.
/// `candidate_tokens` retains ranking's whole-unit estimates, while
/// `included_tokens` is the exact rendered cost charged by packing.
fn build_omission_summary(
    ranked: &[RankedCandidate],
    items: &[ContextItem],
    omitted: usize,
) -> OmissionSummary {
    let weakest_included_score = items
        .iter()
        .map(|item| item.score)
        .reduce(f32::min)
        .unwrap_or(0.0);
    let candidate_tokens = ranked.iter().map(|candidate| candidate.token_count).sum();
    let included_tokens = items.iter().map(|item| item.token_count).sum();
    OmissionSummary {
        omitted,
        weakest_included_score,
        coverage: Coverage {
            candidates: ranked.len(),
            included: ranked.len() - omitted,
            candidate_tokens,
            included_tokens,
        },
    }
}

/// Build the `ContextItem` for one candidate, dispatching on its channel.
///
/// Dispatch is on `candidate.channel`, never on which lookup map hits first —
/// a channel is authoritative about what its own ids mean. Knowledge chunk
/// ids have the form `<path>#<heading>#<occurrence>`; source node ids have
/// the form `<path>#<kind>:<scope>`. Those are disjoint id spaces today, so
/// trying both maps and taking whichever hits would not currently misfire.
/// But `fuse` keys its accumulator by `ChunkId` across channels, so if a
/// future change ever let the two channels produce a colliding id, a
/// both-maps dispatch would silently consult whichever map happened to hit
/// first instead of the one `candidate.channel` actually names, and hide the
/// bug behind a plausible-looking item. Keying off `channel` makes that class
/// of mistake unreachable rather than merely untested.
fn build_item(
    candidate: &RankedCandidate,
    chunks: &BTreeMap<&str, &KnowledgeChunk>,
    nodes: &BTreeMap<&str, &SourceNode>,
    terms: &[String],
    representation: RequiredRepresentation,
) -> Option<ContextItem> {
    match candidate.channel {
        Channel::Knowledge => chunks
            .get(candidate.id.as_str())
            .map(|chunk| build_chunk_item(candidate, chunk, terms, representation)),
        Channel::Source => nodes
            .get(candidate.id.as_str())
            .map(|node| build_source_item(candidate, node, terms, representation)),
    }
}

/// What one budget-constrained walk of the fused list produced.
///
/// Carries no running token total: every fit decision recomputes one from the
/// items and unmet lines it would land with (see [`tentative_total`]), and the
/// number the pack publishes is [`ContextPack::recompute_estimate`]'s. A
/// second definition of "the total so far" is exactly what a later change
/// would wire back into a budget decision.
struct Selection {
    items: Vec<ContextItem>,
    omitted: usize,
    unmet_required: Vec<UnmetRequirement>,
    superseded: BTreeSet<String>,
}

/// Walk the fused list in order, taking whole items while they fit the budget.
///
/// Besides not fitting, two things are skipped rather than taken: a candidate
/// with no backing chunk or node, and a tier-1 summary whose tier-2 detail this
/// pack already carries and which the caller did not name outright (see
/// `twins`). All three are counted in `omitted`, the way the prompt hook
/// folds its dedupe drops into the same figure
/// (`commands/hook/user_prompt_compose.rs:146-165`) — the Knowledge Brief's
/// "Omitted: N weaker matches" line would otherwise tell the reader they were
/// handed everything retrieval found.
fn select(
    request: &PackRequest,
    ranked: &[RankedCandidate],
    chunks: &BTreeMap<&str, &KnowledgeChunk>,
    nodes: &BTreeMap<&str, &SourceNode>,
) -> Selection {
    let reservation = reserve_within_budget(request, ranked, chunks, nodes);
    let mut selection = Selection {
        items: reservation.items,
        omitted: reservation.omitted,
        unmet_required: reservation.unmet,
        superseded: reservation.superseded,
    };
    select_optional(request, ranked, chunks, nodes, &mut selection);
    selection
}

fn select_optional(
    request: &PackRequest,
    ranked: &[RankedCandidate],
    chunks: &BTreeMap<&str, &KnowledgeChunk>,
    nodes: &BTreeMap<&str, &SourceNode>,
    selection: &mut Selection,
) {
    for candidate in details_before_summaries(ranked) {
        if explicitly_required(candidate) {
            continue;
        }
        if selection.superseded.contains(candidate.id.as_str()) {
            tracing::debug!(
                id = candidate.id.as_str(),
                "tier-1 summary omitted: its tier-2 detail is already in the pack"
            );
            selection.omitted += 1;
            continue;
        }
        let Some(mut item) = build_item(
            candidate,
            chunks,
            nodes,
            &request.surviving_terms,
            RequiredRepresentation::Compact,
        ) else {
            selection.omitted += 1;
            continue;
        };
        stamp_snapshot_caveat(&mut item, request.semantic_freshness.state());
        // The unmet list is final by now — `reserve_within_budget` has run —
        // so this prices against the same chrome the finished pack renders.
        if tentative_total(&selection.items, &selection.unmet_required, &item)
            > request.budget_tokens
        {
            selection.omitted += 1;
            continue;
        }
        if let Some(twin) = knowledge_twin(candidate) {
            selection.superseded.insert(twin);
        }
        selection.items.push(item);
    }
}

/// The frame-plus-items-plus-chrome total if `candidate` were appended to
/// `items` (already-committed pack order) alongside `unmet`.
///
/// Delegates to [`rendered_brief_tokens`] rather than tracking chrome
/// incrementally, so this and [`ContextPack::recompute_estimate`] can never
/// charge different bytes for the same brief — both apply the identical rule
/// for what counts as chrome to the identical item list. Item counts are in
/// the tens, so rebuilding the chrome estimate from scratch for every
/// tentative candidate costs nothing that matters.
///
/// Shared with `pack::required::reserve`, which weighs an explicitly required
/// candidate against the same accumulated `items`/`unmet` before admitting
/// it — the required and optional passes must never price chrome by two
/// different rules.
pub(super) fn tentative_total(
    items: &[ContextItem],
    unmet: &[UnmetRequirement],
    candidate: &ContextItem,
) -> usize {
    rendered_brief_tokens(items.iter().chain(std::iter::once(candidate)), unmet)
}

/// Build a pack from the fused list, within `request.budget_tokens`.
///
/// Every ranked candidate not included is counted as an omission — see
/// `select` for the three reasons one can be.
pub fn pack(
    request: &PackRequest,
    ranked: &[RankedCandidate],
    chunks: &[KnowledgeChunk],
    graph: Option<&ResolvedGraph>,
) -> ContextPack {
    let chunk_lookup: BTreeMap<&str, &KnowledgeChunk> = chunks
        .iter()
        .map(|chunk| (chunk.id.as_str(), chunk))
        .collect();
    let node_lookup: BTreeMap<&str, &SourceNode> = graph
        .into_iter()
        .flat_map(|graph| graph.nodes())
        .map(|node| (node.id.as_str(), node))
        .collect();
    let ranked = apply_caveat_factors(ranked, &node_lookup, request.semantic_freshness.state());
    // The `Literal text:` line is chrome outside every item: hold its cost back
    // so `recompute_estimate`, which charges it, still lands within the budget.
    let selection_request = PackRequest {
        budget_tokens: request
            .budget_tokens
            .saturating_sub(request.text_search.as_ref().map_or(0, literal_text_tokens)),
        ..request.clone()
    };
    let selected = select(&selection_request, &ranked, &chunk_lookup, &node_lookup);

    let omitted_summary = build_omission_summary(&ranked, &selected.items, selected.omitted);
    let mut pack = ContextPack {
        query: request.query.clone(),
        scope: request.scope.clone(),
        budget_tokens: request.budget_tokens,
        // Filled in by `recompute_estimate` below; the packer keeps no
        // running total of its own (see [`Selection`]).
        estimated_tokens: 0,
        structural_freshness: request.structural_freshness.clone(),
        semantic_freshness: request.semantic_freshness.clone(),
        items: selected.items,
        unmet_required: selected.unmet_required,
        omitted: omitted_summary,
        dropped_terms: request.dropped_terms.clone(),
        degraded: request.degraded.clone(),
        text_search: request.text_search.clone(),
    };
    pack.recompute_estimate();
    pack
}
