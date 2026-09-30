//! Shared type contract for the deterministic context retrieval subsystem.
//!
//! Every module under [`crate::context`] — and the knowledge chunker/catalog in
//! [`crate::fs::knowledge`] — compiles against the types defined here. There is
//! deliberately no model call and no network access anywhere in this subsystem:
//! a [`ContextPack`] is a pure function of the bytes on disk plus the query.
//!
//! Token counts are **estimates** ([`estimate_tokens`]), never tokenizer output.
//! They are used only to keep a pack inside its budget, so a conservative
//! four-bytes-per-token approximation is sufficient and keeps the crate free of
//! a tokenizer dependency.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

mod estimate;
mod lifecycle;
mod pack_types;
pub use estimate::*;
pub use lifecycle::*;
pub use pack_types::{ContextItem, ContextPack, TextSearchHint, UnmetRequirement};

/// A retrieval channel a [`ContextItem`] can come from.
///
/// Channels are ranked independently and then fused (see `crate::context::fuse`),
/// so a channel is both "where this came from" and "which rank list it competed in".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// Curated prose under `doc/loom/knowledge/`.
    Knowledge,
    /// The derived source graph (populated by the source-graph stage).
    Source,
}

impl Channel {
    /// Every channel, in fusion order.
    pub fn all() -> &'static [Channel] {
        &[Channel::Knowledge, Channel::Source]
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Channel::Knowledge => "knowledge",
            Channel::Source => "source",
        };
        f.write_str(name)
    }
}

/// What a [`ContextItem`] actually is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    /// One `##` section of a knowledge markdown file.
    KnowledgeChunk,
    /// One node of the derived source graph.
    SourceNode,
}

/// Stable identity of a retrievable unit.
///
/// For knowledge chunks the form is `<relative-path>#<normalized-heading>#<occurrence>`,
/// which is why it is a plain string rather than a structured key: it must survive
/// a round trip through JSON, a CLI argument, and a `--require-id` flag unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChunkId(String);

impl ChunkId {
    pub fn new(id: impl Into<String>) -> Self {
        ChunkId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ChunkId {
    fn from(value: String) -> Self {
        ChunkId(value)
    }
}

impl From<&str> for ChunkId {
    fn from(value: &str) -> Self {
        ChunkId(value.to_string())
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where an item's text physically lives, precise enough for an agent to open it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePointer {
    /// Path relative to the project root.
    pub path: PathBuf,
    /// Normalized heading anchor within the file, empty for a whole-file pointer.
    #[serde(default)]
    pub anchor: String,
    /// 1-indexed inclusive start line, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_start: Option<usize>,
    /// 1-indexed inclusive end line, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_end: Option<usize>,
}

/// Why an item was selected. Every scoring contribution names one of these so
/// `--explain` can attribute a score instead of asserting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionReason {
    /// The caller named this id explicitly.
    ExplicitId,
    /// The query contained this item's source path verbatim.
    ExactPath,
    /// The query contained one of this item's symbols verbatim.
    ExactSymbol,
    /// A directly linked neighbour of an already-selected item.
    LinkedFrom,
    /// Referenced by a stage this query's stage depends on.
    StageDependency,
    /// A source-graph neighbour of an already-selected item.
    GraphNeighbor,
    /// The query asked what a symbol is or does, and this node is that symbol.
    SymbolQuestion,
    /// BM25 lexical overlap with the query.
    Lexical,
}

impl fmt::Display for SelectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            SelectionReason::ExplicitId => "explicit-id",
            SelectionReason::ExactPath => "exact-path",
            SelectionReason::ExactSymbol => "exact-symbol",
            SelectionReason::LinkedFrom => "linked-from",
            SelectionReason::StageDependency => "stage-dependency",
            SelectionReason::GraphNeighbor => "graph-neighbor",
            SelectionReason::SymbolQuestion => "symbol-question",
            SelectionReason::Lexical => "lexical",
        };
        f.write_str(name)
    }
}

/// How much to trust an item's relevance.
///
/// Derived from *which* reasons fired, not from a probability: an exact id or
/// path match is high, a purely lexical hit is low.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    /// Classify from the reasons that fired. Exact identity beats structure,
    /// structure beats lexical overlap.
    pub fn from_reasons(reasons: &[SelectionReason]) -> Self {
        if reasons.iter().any(|reason| {
            matches!(
                reason,
                SelectionReason::ExplicitId
                    | SelectionReason::ExactPath
                    | SelectionReason::ExactSymbol
            )
        }) {
            Confidence::High
        } else if reasons.iter().any(|reason| {
            matches!(
                reason,
                SelectionReason::LinkedFrom | SelectionReason::StageDependency
            )
        }) {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    }
}

// Derived-layer currency is its own small domain and lives in a sibling module;
// re-exported here so the shared contract still names every retrieval type.
pub use crate::context::freshness::Freshness;

/// What fraction of the candidate set made it into the pack.
///
/// Reported on every pack so a caller can tell a complete answer from a
/// budget-truncated one without re-running retrieval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// Candidates the ranker considered.
    pub candidates: usize,
    /// Candidates that fit the budget.
    pub included: usize,
    /// Estimated tokens across all candidates.
    pub candidate_tokens: usize,
    /// Estimated tokens across included items.
    pub included_tokens: usize,
}

/// What the packer left out, and how close it came to including it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OmissionSummary {
    /// Number of ranked candidates that did not fit.
    pub omitted: usize,
    /// Score of the lowest-scoring item that *did* fit — the cut line.
    pub weakest_included_score: f32,
    pub coverage: Coverage,
}

// The derived source graph is its own domain and lives in a sibling module;
// re-exported here so the shared contract still names every retrieval type.
pub use crate::context::source_graph::{
    EdgeProvenance, FileCoverage, ImportBinding, NodeLanguage, SourceEdge, SourceEdgeKind,
    SourceNode, SourceNodeKind, Span,
};

/// One `##` section of a knowledge markdown file — the atom of retrieval.
///
/// Produced by `crate::fs::knowledge::chunker`, which re-exports this type so
/// chunker callers need not reach into `crate::context`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChunk {
    /// `<relative-path>#<normalized-heading>#<occurrence>`.
    pub id: String,
    /// Path relative to the knowledge root.
    pub file: PathBuf,
    /// Normalized heading, empty for the preamble chunk.
    pub anchor: String,
    /// Heading text as written, empty for the preamble chunk.
    pub heading: String,
    /// Section body including the heading line.
    pub body: String,
    /// `sha256:<hex>` over `body`.
    pub content_hash: String,
    /// Estimated, not tokenized — see [`estimate_tokens`].
    pub estimated_tokens: usize,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Knowledge category directory name, when the file sits under one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Backticked source paths mentioned in the body.
    #[serde(default)]
    pub source_paths: Vec<String>,
    /// Backticked identifiers mentioned in the body.
    #[serde(default)]
    pub symbols: Vec<String>,
    /// `[text](target.md)` pairs as `(text, target)`.
    #[serde(default)]
    pub links: Vec<(String, String)>,
    #[serde(default)]
    pub state: LifecycleState,
}
