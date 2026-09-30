//! What each [`QueryIntent`] changes about source ranking.
//!
//! - `SymbolQuestion` admits the node the question names, at low confidence,
//!   which the ordinary candidacy rule refuses for a one-word name. A name that
//!   many nodes share is too ambiguous to name one, so it admits nothing.
//! - `Relationship` seeds the named node and admits its direct neighbours in
//!   the asked direction, each carrying the edge that admitted it (see
//!   `relation`).
//! - `Literal` halves every source lexical contribution. Only the scorer can
//!   tell a node's lexical score from its rungs, so it reads
//!   [`lexical_weight`] rather than being routed after the fact.
//! - `General` changes nothing.

use super::intent::{classify, QueryIntent};
use super::paths::apply_test_path_factor;
use super::{estimate_node_tokens, score_nodes, sorted_candidates, ScoredNode};
use crate::context::config::RetrievalConfig;
use crate::context::graph_store::ResolvedGraph;
use crate::context::lexical::ExactGate;
use crate::context::rank::{LexicalCorpus, RankQuery, RankedCandidate, BOOST_EXACT_SYMBOL};
use crate::context::schema::{Channel, ChunkId, Confidence, SelectionReason, SourceNode};

mod relation;

/// Factor on every source lexical contribution for a literal query: the quoted
/// text is what was asked for, and no node body is indexed to match it.
const LITERAL_LEXICAL_FACTOR: f32 = 0.5;

/// Share of [`BOOST_EXACT_SYMBOL`] a symbol-question node scores when it has no
/// lexical score of its own.
const SYMBOL_QUESTION_BOOST_FACTOR: f32 = 0.25;

/// Most nodes one symbol question may admit. A name more nodes than this carry
/// (`new`, `len`) says nothing about which one was meant.
const MAX_SYMBOL_QUESTION_MATCHES: usize = 3;

/// What routing reads besides the candidate list.
pub(super) struct RouteInputs<'a, 'r> {
    pub(super) graph: &'a ResolvedGraph,
    /// The ranked nodes, in corpus order: a node's position is its lexical index.
    pub(super) nodes: &'r [&'a SourceNode],
    pub(super) corpus: &'r LexicalCorpus,
    pub(super) config: &'r RetrievalConfig,
}

/// Factor the scorer applies to a node's lexical contribution: `1.0` leaves
/// every non-literal score exactly as it was.
pub(super) fn lexical_weight(intent: &QueryIntent) -> f32 {
    match intent {
        QueryIntent::Literal { .. } => LITERAL_LEXICAL_FACTOR,
        _ => 1.0,
    }
}

/// Add the candidates `intent` asks for to `scored`, merging into a candidate
/// already present rather than duplicating it. Returns the estimated rendered
/// tokens of the neighbours admitted, which count against
/// `MAX_EXPANDED_TOKENS` together with the ones expansion admits afterwards.
pub(super) fn route<'a>(
    intent: &QueryIntent,
    scored: &mut Vec<ScoredNode<'a>>,
    inputs: &RouteInputs<'a, '_>,
) -> usize {
    match intent {
        QueryIntent::SymbolQuestion { symbol } => {
            admit_symbol_question(symbol, scored, inputs);
            0
        }
        QueryIntent::Relationship { direction, symbol } => {
            relation::admit_relationship(*direction, symbol, scored, inputs)
        }
        QueryIntent::Literal { .. } | QueryIntent::General => 0,
    }
}

/// Score every node, let the query's intent admit or reweight candidates, and
/// put the result in seed-strength order. Also returns the tokens routing spent
/// on neighbours.
pub(super) fn score_and_route<'a>(
    query: &RankQuery,
    graph: &'a ResolvedGraph,
    nodes: &[&'a SourceNode],
    corpus: &LexicalCorpus,
    config: &RetrievalConfig,
) -> (Vec<RankedCandidate>, usize) {
    let gate = ExactGate::new(
        &query.text,
        &corpus.document_frequencies,
        config.df_ident_max,
    );
    let intent = classify(&query.text);
    let weight = lexical_weight(&intent);
    let mut scored = score_nodes(query, nodes, corpus, &gate, config, weight);
    let inputs = RouteInputs {
        graph,
        nodes,
        corpus,
        config,
    };
    let routed_tokens = route(&intent, &mut scored, &inputs);
    (sorted_candidates(scored), routed_tokens)
}

/// Admit every node `symbol` names that is not a candidate already, unless
/// more than [`MAX_SYMBOL_QUESTION_MATCHES`] nodes carry the name. A node that
/// is a candidate has earned its place on stronger evidence and keeps it as is.
fn admit_symbol_question<'a>(
    symbol: &str,
    scored: &mut Vec<ScoredNode<'a>>,
    inputs: &RouteInputs<'a, '_>,
) {
    let named = named_nodes(symbol, inputs.nodes);
    if named.len() > MAX_SYMBOL_QUESTION_MATCHES {
        return;
    }
    for (index, node) in named {
        if find_mut(scored, &node.id).is_some() {
            continue;
        }
        let (lexical, _) = inputs.corpus.score(inputs.nodes.len() as f32, index);
        let base = if lexical > 0.0 {
            lexical
        } else {
            BOOST_EXACT_SYMBOL * SYMBOL_QUESTION_BOOST_FACTOR
        };
        let score = apply_test_path_factor(node, base, inputs.config);
        let candidate = RankedCandidate {
            matched_term_count: 1,
            confidence_ceiling: Some(Confidence::Low),
            ..source_candidate(node, score, SelectionReason::SymbolQuestion)
        };
        scored.push(scored_node(node, candidate));
    }
}

/// Nodes `symbol` names, beside their corpus index. The symbol's `::` or `.`
/// segments must equal the node scope's last segments, so `tokenize` names
/// every `tokenize` and `Lexer::tokenize` only the one inside `Lexer`.
fn named_nodes<'a>(symbol: &str, nodes: &[&'a SourceNode]) -> Vec<(usize, &'a SourceNode)> {
    let segments: Vec<&str> = symbol
        .split("::")
        .flat_map(|part| part.split('.'))
        .collect();
    nodes
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, node)| scope_ends_with(&node.scope, &segments))
        .collect()
}

fn scope_ends_with(scope: &[String], segments: &[&str]) -> bool {
    scope.len() >= segments.len()
        && scope
            .iter()
            .rev()
            .zip(segments.iter().rev())
            .all(|(scope, segment)| scope.as_str() == *segment)
}

fn find_mut<'s, 'a>(scored: &'s mut [ScoredNode<'a>], id: &str) -> Option<&'s mut ScoredNode<'a>> {
    scored
        .iter_mut()
        .find(|scored| scored.candidate.id.as_str() == id)
}

fn source_candidate(node: &SourceNode, score: f32, reason: SelectionReason) -> RankedCandidate {
    RankedCandidate {
        id: ChunkId::from(node.id.as_str()),
        channel: Channel::Source,
        score,
        reasons: vec![reason],
        token_count: estimate_node_tokens(node),
        matched_term_count: 0,
        confidence_ceiling: None,
        via: None,
    }
}

fn scored_node<'a>(node: &'a SourceNode, candidate: RankedCandidate) -> ScoredNode<'a> {
    ScoredNode {
        candidate,
        order: (node.path.as_path(), node.span.line_start),
    }
}
