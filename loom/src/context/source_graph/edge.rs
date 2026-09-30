//! Edge types for the derived source graph: [`SourceEdge`], its
//! [`SourceEdgeKind`], and [`EdgeProvenance`].

use serde::{Deserialize, Serialize};

use super::{
    syntax_confidence, Span, IMPORT_CONFIDENCE, LOCAL_NAME_CONFIDENCE, MAX_SYNTAX_CONFIDENCE,
    RECEIVER_CONFIDENCE, STRUCTURAL_CONFIDENCE, UNIQUE_NAME_CONFIDENCE, UNRESOLVED_TARGET,
};

/// The relationship a [`SourceEdge`] encodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceEdgeKind {
    /// Lexical containment: file contains symbol, type contains method.
    Contains,
    /// An import, `use`, or `require` of another module.
    Imports,
    /// A call expression.
    Calls,
    /// A non-call mention of an identifier.
    References,
    /// A trait/interface implementation.
    Implements,
    /// Subclassing or trait supertrait.
    Extends,
}

impl SourceEdgeKind {
    /// Stable lowercase name used in CLI output and fixture JSON.
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceEdgeKind::Contains => "contains",
            SourceEdgeKind::Imports => "imports",
            SourceEdgeKind::Calls => "calls",
            SourceEdgeKind::References => "references",
            SourceEdgeKind::Implements => "implements",
            SourceEdgeKind::Extends => "extends",
        }
    }
}

impl std::fmt::Display for SourceEdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an edge's claim comes from. The graph is never asserted to be
/// complete; this is how a consumer tells a fact from a guess.
///
/// Variants are declared strongest first, so the derived `Ord` agrees with
/// [`EdgeProvenance::rank`] reversed. Numeric confidence is an evidence ranking,
/// not a calibrated probability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeProvenance {
    /// Containment: both endpoints are declarations the grammar placed in one
    /// file.
    Structural,
    /// Bound by a compiler or language server. Reserved: nothing emits this.
    Compiler,
    /// A `self`/`this`/`Self`/`$this`/`static` call bound to a member of the
    /// enclosing type.
    Receiver,
    /// Bound through an import, alias, module-qualified path, or package or
    /// namespace scope to exactly one definition.
    Import,
    /// Same-file spelling with exactly one in-scope definition.
    LocalName,
    /// The only same-family definition of the name in the graph, with no
    /// stronger evidence and no refusal rule firing.
    UniqueName,
    /// Captured at a site; the target is unresolved or ambiguous.
    Syntax,
}

impl EdgeProvenance {
    /// Stable lowercase name used in CLI output and fixture JSON.
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeProvenance::Structural => "structural",
            EdgeProvenance::Compiler => "compiler",
            EdgeProvenance::Receiver => "receiver",
            EdgeProvenance::Import => "import",
            EdgeProvenance::LocalName => "local-name",
            EdgeProvenance::UniqueName => "unique-name",
            EdgeProvenance::Syntax => "syntax",
        }
    }

    /// The confidence an edge of this class carries once bound. `Syntax`
    /// reports its ceiling; the actual value depends on the edge kind (see
    /// [`syntax_confidence`]).
    pub fn ceiling(self) -> f32 {
        match self {
            EdgeProvenance::Structural => STRUCTURAL_CONFIDENCE,
            EdgeProvenance::Compiler => 1.0,
            EdgeProvenance::Receiver => RECEIVER_CONFIDENCE,
            EdgeProvenance::Import => IMPORT_CONFIDENCE,
            EdgeProvenance::LocalName => LOCAL_NAME_CONFIDENCE,
            EdgeProvenance::UniqueName => UNIQUE_NAME_CONFIDENCE,
            EdgeProvenance::Syntax => MAX_SYNTAX_CONFIDENCE,
        }
    }

    /// Strength order for "weakest provenance" reporting: `Structural` is 6,
    /// `Syntax` is 0.
    pub fn rank(self) -> u8 {
        match self {
            EdgeProvenance::Structural => 6,
            EdgeProvenance::Compiler => 5,
            EdgeProvenance::Receiver => 4,
            EdgeProvenance::Import => 3,
            EdgeProvenance::LocalName => 2,
            EdgeProvenance::UniqueName => 1,
            EdgeProvenance::Syntax => 0,
        }
    }
}

impl std::fmt::Display for EdgeProvenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `"{path}@{start_byte}-{end_byte}"`: the id of one reference site. Stable
/// within one snapshot and never persisted separately, because an edge lives in
/// the entry of the file it was extracted from and that entry's key is `path`.
pub fn site_id(path: &str, span: &Span) -> String {
    format!("{path}@{}-{}", span.start_byte, span.end_byte)
}

/// One directed edge of the derived source graph.
///
/// Construct through [`SourceEdge::structural`], [`SourceEdge::syntax`] or
/// [`SourceEdge::bound`] so the provenance/confidence invariant cannot be
/// violated by accident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceEdge {
    /// [`crate::context::source_graph::SourceNode::id`] of the origin.
    pub from: String,
    /// [`crate::context::source_graph::SourceNode::id`] of the target, or
    /// [`UNRESOLVED_TARGET`].
    pub to: String,
    pub kind: SourceEdgeKind,
    pub provenance: EdgeProvenance,
    /// How much to trust this edge, in `0.0..=1.0`.
    pub confidence: f32,
    /// The identifier as written at the call/import site. Kept so an
    /// unresolved edge still names what it was looking for.
    #[serde(default)]
    pub symbol: String,
    /// Every reference site, sorted by `start_byte` and deduplicated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<Span>,
    /// Sorted node ids an ambiguous `Syntax` edge could mean, at most
    /// [`crate::context::source_graph::MAX_CANDIDATES`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    /// Receiver text of a member call (`self`, `obj`, `ns`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
}

impl SourceEdge {
    fn new(
        from: String,
        to: String,
        kind: SourceEdgeKind,
        provenance: EdgeProvenance,
        confidence: f32,
        symbol: String,
    ) -> Self {
        SourceEdge {
            from,
            to,
            kind,
            provenance,
            confidence,
            symbol,
            sites: Vec::new(),
            candidates: Vec::new(),
            receiver: None,
        }
    }

    /// A `Contains` edge whose endpoints the grammar placed in one file.
    pub fn structural(
        from: impl Into<String>,
        to: impl Into<String>,
        symbol: impl Into<String>,
    ) -> Self {
        SourceEdge::new(
            from.into(),
            to.into(),
            SourceEdgeKind::Contains,
            EdgeProvenance::Structural,
            STRUCTURAL_CONFIDENCE,
            symbol.into(),
        )
    }

    /// An edge captured at `site` whose target is not resolved.
    ///
    /// `confidence` is clamped to [`MAX_SYNTAX_CONFIDENCE`]: a syntax edge can
    /// never present itself as bound. Callers pass [`syntax_confidence`].
    pub fn syntax(
        from: impl Into<String>,
        kind: SourceEdgeKind,
        symbol: impl Into<String>,
        site: Span,
        confidence: f32,
    ) -> Self {
        let mut edge = SourceEdge::new(
            from.into(),
            UNRESOLVED_TARGET.to_string(),
            kind,
            EdgeProvenance::Syntax,
            confidence.clamp(0.0, MAX_SYNTAX_CONFIDENCE),
            symbol.into(),
        );
        edge.sites.push(site);
        edge
    }

    /// An edge already bound to `to` at extraction time, with the confidence
    /// ceiling of `provenance`. Refused (debug-asserted) for `Structural` and
    /// `Syntax`, which have their own constructors, and for the reserved
    /// `Compiler`, whose ceiling of 1.0 only containment may reach.
    pub fn bound(
        from: impl Into<String>,
        to: impl Into<String>,
        kind: SourceEdgeKind,
        symbol: impl Into<String>,
        site: Span,
        provenance: EdgeProvenance,
    ) -> Self {
        debug_assert!(
            !matches!(
                provenance,
                EdgeProvenance::Structural | EdgeProvenance::Compiler | EdgeProvenance::Syntax
            ),
            "SourceEdge::bound is not for {provenance}"
        );
        let mut edge = SourceEdge::new(
            from.into(),
            to.into(),
            kind,
            provenance,
            provenance.ceiling(),
            symbol.into(),
        );
        edge.sites.push(site);
        edge
    }

    /// Record the receiver text of a member call.
    pub fn with_receiver(mut self, receiver: impl Into<String>) -> Self {
        self.receiver = Some(receiver.into());
        self
    }

    /// Record the ids an ambiguous edge could mean.
    pub fn with_candidates(mut self, candidates: Vec<String>) -> Self {
        self.candidates = candidates;
        self
    }

    /// Number of reference sites this edge covers.
    pub fn site_count(&self) -> usize {
        self.sites.len()
    }

    /// True when this edge does not name a resolved target.
    pub fn is_unresolved(&self) -> bool {
        self.to == UNRESOLVED_TARGET
    }

    /// Point an unresolved `Syntax` edge at a target that whole-graph
    /// resolution found, raising confidence to the ceiling of `provenance`.
    ///
    /// Returns `false` and changes nothing unless the edge is an unresolved
    /// `Syntax` edge and `provenance` is `Receiver`, `Import` or `UniqueName`.
    /// Resolution refines a gap: it never touches a structural edge, never
    /// retargets an already-bound edge, and never claims `LocalName`, which only
    /// extraction can prove.
    pub fn bind(&mut self, target: impl Into<String>, provenance: EdgeProvenance) -> bool {
        let bindable = matches!(
            provenance,
            EdgeProvenance::Receiver | EdgeProvenance::Import | EdgeProvenance::UniqueName
        );
        if !bindable || self.provenance != EdgeProvenance::Syntax || !self.is_unresolved() {
            return false;
        }
        self.to = target.into();
        self.provenance = provenance;
        self.confidence = provenance.ceiling();
        self.candidates.clear();
        true
    }

    /// Restore the extraction-time `Syntax` state: unresolved target, the
    /// confidence of [`syntax_confidence`], no candidates. The incremental
    /// relink uses it before rebinding.
    pub fn unbind(&mut self) {
        self.to = UNRESOLVED_TARGET.to_string();
        self.provenance = EdgeProvenance::Syntax;
        self.confidence = syntax_confidence(self.kind);
        self.candidates.clear();
    }
}
