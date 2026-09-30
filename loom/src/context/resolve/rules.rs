//! The ordered resolution rules for one edge (design section 7).
//!
//! Rules 1 (qualified spelling), 5 (package scope) and 7 (unique name) live
//! here; rules 2, 4 and 6 read the file's import bindings (`bindings`), and
//! rule 3 joins a type's parts across files (`receivers`). Each rule either
//! decides the edge or hands it to the next; a refusal is carried forward and
//! only ever withholds rule 7's bind.
//!
//! Any change to what rules 1-7 decide bumps `RESOLVER_VERSION`, so a view
//! resolved by the previous rules is rebuilt rather than served.

use std::collections::BTreeSet;
use std::path::Path;

use crate::context::extract::dialect::{dialect_for_path, DialectSpec};
use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::source_graph::{EdgeProvenance, SourceEdge, SourceEdgeKind, MAX_CANDIDATES};

use super::bindings::{self, Step};
use super::paths::{import_candidates, PathIndex};
use super::receivers;
use super::symbols::SymbolIndex;

/// Families whose files share a package scope beyond imports: Go and Java by
/// directory, C# and PHP by declared namespace (`PathIndex::package_files`).
/// Asking any other family would only record a `pathset:` key for a lookup
/// that cannot find anything.
const PACKAGE_FAMILIES: [&str; 4] = ["go", "java", "csharp", "php"];

/// What resolution decided for one edge.
pub(super) enum Outcome {
    /// Exactly one target, with the evidence class that found it.
    Bound(String, EdgeProvenance),
    /// Two to [`MAX_CANDIDATES`] targets, sorted; or a lone target a rule may
    /// name but never bind.
    Candidates(Vec<String>),
    /// Nothing the evidence supports.
    Unresolved,
}

/// An edge's outcome and every lookup key its rules consulted.
pub(super) struct Resolution {
    pub(super) outcome: Outcome,
    pub(super) keys: BTreeSet<String>,
}

/// The whole-graph indexes every rule reads, built once per resolution.
pub(super) struct Indexes {
    pub(super) symbols: SymbolIndex,
    pub(super) paths: PathIndex,
}

impl Indexes {
    pub(super) fn build(graph: &ResolvedGraph) -> Self {
        Indexes {
            symbols: SymbolIndex::build(graph),
            paths: PathIndex::build(graph),
        }
    }
}

/// One edge together with everything its rules read.
pub(super) struct Site<'a> {
    pub(super) edge: &'a SourceEdge,
    /// Path of the file the edge was extracted from.
    pub(super) file: &'a str,
    pub(super) entry: &'a FileEntry,
    pub(super) dialect: &'static DialectSpec,
    pub(super) indexes: &'a Indexes,
}

impl Site<'_> {
    pub(super) fn family(&self) -> &'static str {
        self.dialect.family
    }

    pub(super) fn symbols(&self) -> &SymbolIndex {
        &self.indexes.symbols
    }

    /// The outcome of `ids` as targets of this edge: one binds with
    /// `provenance`, more become candidates.
    pub(super) fn decide(&self, mut ids: Vec<String>, provenance: EdgeProvenance) -> Outcome {
        if ids.len() == 1 && ids[0] != self.edge.from {
            return Outcome::Bound(ids.remove(0), provenance);
        }
        self.candidates_only(ids)
    }

    /// The outcome of `ids` as targets this edge may name but never bind: one
    /// to [`MAX_CANDIDATES`] of them become candidates. A lone candidate that
    /// is the edge's own origin resolves nothing.
    pub(super) fn candidates_only(&self, ids: Vec<String>) -> Outcome {
        let lone_origin = ids.len() == 1 && ids[0] == self.edge.from;
        if ids.is_empty() || lone_origin || ids.len() > MAX_CANDIDATES {
            return Outcome::Unresolved;
        }
        Outcome::Candidates(ids)
    }

    /// Candidates named like `name` in the edge's family, for a call no rule
    /// may bind: a dynamic receiver, or an edge a refusal fired on.
    pub(super) fn name_candidates(&self, name: &str, keys: &mut BTreeSet<String>) -> Outcome {
        let ids = self
            .symbols()
            .definitions_in_family(self.family(), name, keys);
        self.candidates_only(ids)
    }
}

/// Whether resolution acts on `edge`: an unresolved `Syntax` call, reference
/// or import that extraction left without candidates. A candidate set found at
/// extraction is same-file ambiguity, which is final.
pub(crate) fn eligible(edge: &SourceEdge) -> bool {
    edge.provenance == EdgeProvenance::Syntax
        && edge.is_unresolved()
        && edge.candidates.is_empty()
        && !edge.symbol.is_empty()
        && matches!(
            edge.kind,
            SourceEdgeKind::Calls | SourceEdgeKind::References | SourceEdgeKind::Imports
        )
}

/// Run the rules for `edge`, extracted from the file at `file` whose entry is
/// `entry`. An edge of a file no dialect claims resolves nothing.
pub(super) fn resolve(
    edge: &SourceEdge,
    file: &str,
    entry: &FileEntry,
    indexes: &Indexes,
) -> Resolution {
    let mut keys = BTreeSet::new();
    let outcome = match dialect_for_path(Path::new(file)) {
        Some(dialect) => {
            let site = Site {
                edge,
                file,
                entry,
                dialect,
                indexes,
            };
            match edge.kind {
                SourceEdgeKind::Imports => import_edge(&site, &mut keys),
                _ => call(&site, &mut keys),
            }
        }
        None => Outcome::Unresolved,
    };
    Resolution { outcome, keys }
}

/// An import statement names the files its module spec matches: one binds,
/// more become candidates.
fn import_edge(site: &Site, keys: &mut BTreeSet<String>) -> Outcome {
    let files = import_candidates(&site.edge.symbol, site.file, &site.indexes.paths, keys);
    site.decide(files, EdgeProvenance::Import)
}

/// A call or reference: a member call goes to rule 2 or 3 by its receiver, a
/// qualified spelling to rule 1, a bare name through rules 4 to 7.
fn call(site: &Site, keys: &mut BTreeSet<String>) -> Outcome {
    if let Some(receiver) = site.edge.receiver.as_deref() {
        return if site.dialect.self_receivers.contains(&receiver) {
            receivers::self_call(site, keys)
        } else {
            bindings::member_call(site, receiver, keys)
        };
    }
    let symbol = site.edge.symbol.as_str();
    let separator = if symbol.contains("::") { "::" } else { "." };
    match symbol.rsplit_once(separator) {
        Some((qualifier, name)) if !qualifier.is_empty() && !name.is_empty() => {
            qualified(site, separator, qualifier, name, keys)
        }
        _ => bare(site, symbol, keys),
    }
}

/// Rule 1: the qualified spelling against node scopes, longest first, then the
/// qualifier as a module path naming the files `name` is looked up in. A
/// qualifier that names no file may still be an import's local name (rule 4);
/// otherwise it is a call out of the graph and stays a gap, because every
/// same-named definition here is then a namesake rather than a candidate.
fn qualified(
    site: &Site,
    separator: &str,
    qualifier: &str,
    name: &str,
    keys: &mut BTreeSet<String>,
) -> Outcome {
    let family = site.family();
    let segments: Vec<&str> = site.edge.symbol.split(separator).collect();
    for start in 0..segments.len() - 1 {
        let spelling = segments[start..].join("::");
        if !site.symbols().lookup(family, &spelling, keys).is_empty() {
            let ids = site
                .symbols()
                .definitions_in_family(family, &spelling, keys);
            return site.decide(ids, EdgeProvenance::Import);
        }
    }
    let paths = &site.indexes.paths;
    let files = paths.module_files(qualifier, site.file, site.dialect, keys);
    if !files.is_empty() {
        let ids = site
            .symbols()
            .definitions_in_files(family, name, &files, keys);
        return site.decide(ids, EdgeProvenance::Import);
    }
    bindings::qualified_import(site, &segments, keys).unwrap_or(Outcome::Unresolved)
}

/// Rules 4 to 7 for a bare name: a named import, the package, the glob
/// imports, then graph-wide uniqueness. Rule 4's external module skips rule 6,
/// and either refusal leaves rule 7 recording candidates instead of binding.
fn bare(site: &Site, name: &str, keys: &mut BTreeSet<String>) -> Outcome {
    let mut refused = false;
    match bindings::named_import(site, name, keys) {
        Step::Decided(outcome) => return outcome,
        Step::Refused => refused = true,
        Step::Next => {}
    }
    if let Some(outcome) = package_scope(site, name, keys) {
        return outcome;
    }
    if !refused {
        match bindings::globs(site, name, keys) {
            Step::Decided(outcome) => return outcome,
            Step::Refused => refused = true,
            Step::Next => {}
        }
    }
    if refused {
        return site.name_candidates(name, keys);
    }
    let ids = site
        .symbols()
        .definitions_in_family(site.family(), name, keys);
    site.decide(ids, EdgeProvenance::UniqueName)
}

/// Rule 5: `name` among the other files of the edge's package. `None` when the
/// package defines nothing by that name.
fn package_scope(site: &Site, name: &str, keys: &mut BTreeSet<String>) -> Option<Outcome> {
    let family = site.family();
    if !PACKAGE_FAMILIES.contains(&family) {
        return None;
    }
    let files = site
        .indexes
        .paths
        .package_files(site.file, site.dialect, keys);
    let ids = site
        .symbols()
        .definitions_in_files(family, name, &files, keys);
    (!ids.is_empty()).then(|| site.decide(ids, EdgeProvenance::Import))
}
