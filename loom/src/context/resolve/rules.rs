//! The ordered resolution rules for one edge (design section 7).
//!
//! Rules 1 (qualified spelling), 5 (package scope) and 7 (unique name) live
//! here; rules 2, 4 and 6 read the file's import bindings (`bindings`), and
//! rule 3 joins a type's parts across files (`receivers`). Each rule either
//! decides the edge or hands it to the next; a refusal is carried forward and
//! only ever withholds rule 7's bind.
//!
//! Any change to what rules 1-7 decide bumps `RESOLVER_VERSION`
//! (`context/view/identity.rs`), so a view resolved by the previous rules is
//! rebuilt rather than served.

use std::collections::BTreeSet;
use std::path::Path;

use crate::context::extract::dialect::{dialect_for_path, DialectSpec};
use crate::context::graph_store::{FileEntry, ResolvedGraph};
use crate::context::source_graph::{EdgeProvenance, SourceEdge, SourceEdgeKind, MAX_CANDIDATES};

use super::bindings::{self, relative_spec, Step};
use super::paths::{import_candidates, PathIndex};
use super::receivers;
use super::record::{name_key, record_file};
use super::symbols::SymbolIndex;

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

/// The whole-graph indexes every rule reads, built once per resolution, and
/// the graph they were built from.
pub(super) struct Indexes<'g> {
    pub(super) symbols: SymbolIndex,
    pub(super) paths: PathIndex,
    graph: &'g ResolvedGraph,
}

impl<'g> Indexes<'g> {
    pub(super) fn build(graph: &'g ResolvedGraph) -> Self {
        Indexes {
            symbols: SymbolIndex::build(graph),
            paths: PathIndex::build(graph),
            graph,
        }
    }

    /// The entry of the file at `path`, for a rule that reads another file's
    /// import bindings or edges. Records the keys any change to that file
    /// touches.
    pub(super) fn entry(&self, path: &str, keys: &mut BTreeSet<String>) -> Option<&'g FileEntry> {
        record_file(path, keys);
        self.graph.files.get(path)
    }
}

/// One edge together with everything its rules read.
pub(super) struct Site<'a> {
    pub(super) edge: &'a SourceEdge,
    /// Path of the file the edge was extracted from.
    pub(super) file: &'a str,
    pub(super) entry: &'a FileEntry,
    pub(super) dialect: &'static DialectSpec,
    pub(super) indexes: &'a Indexes<'a>,
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
    pub(super) fn decide(&self, ids: Vec<String>, provenance: EdgeProvenance) -> Outcome {
        let mut ids = self.targets(ids);
        if ids.len() == 1 && ids[0] != self.edge.from {
            return Outcome::Bound(ids.remove(0), provenance);
        }
        self.listed(ids)
    }

    /// The outcome of `ids` as targets this edge may name but never bind: one
    /// to [`MAX_CANDIDATES`] of them become candidates. A lone candidate that
    /// is the edge's own origin resolves nothing.
    pub(super) fn candidates_only(&self, ids: Vec<String>) -> Outcome {
        self.listed(self.targets(ids))
    }

    /// `ids` narrowed to what this edge's kind can land on: a call never
    /// lands on a file, nor on a type beside its own constructor.
    fn targets(&self, ids: Vec<String>) -> Vec<String> {
        match self.edge.kind {
            SourceEdgeKind::Calls => self.symbols().callable(ids),
            _ => ids,
        }
    }

    fn listed(&self, ids: Vec<String>) -> Outcome {
        let lone_origin = ids.len() == 1 && ids[0] == self.edge.from;
        if ids.is_empty() || lone_origin || ids.len() > MAX_CANDIDATES {
            return Outcome::Unresolved;
        }
        Outcome::Candidates(ids)
    }

    /// Candidates named like `name` in the edge's family, for a member call no
    /// rule may bind: a dynamic receiver, or a self call whose type has no such
    /// member.
    pub(super) fn name_candidates(&self, name: &str, keys: &mut BTreeSet<String>) -> Outcome {
        let ids = self
            .symbols()
            .definitions_in_family(self.family(), name, keys);
        self.candidates_only(ids)
    }

    /// `ids` a bare name can reach from this edge's dialect. One whose bare
    /// calls never reach members (design 6.2, rule 3) drops every member of a
    /// type, as extraction does: there a method is only ever named through a
    /// receiver or a qualifier, so a bare `run()` is never a Go method
    /// `Widget::run` or a Rust `W::run`. Whether an id is a member depends on
    /// its owner type's definitions, so the owner's name is recorded.
    pub(super) fn bare_reachable(
        &self,
        mut ids: Vec<String>,
        keys: &mut BTreeSet<String>,
    ) -> Vec<String> {
        if self.dialect.bare_calls_reach_members {
            return ids;
        }
        let symbols = self.symbols();
        let owners = ids.iter().filter_map(|id| symbols.owner(id));
        keys.extend(owners.map(|owner| name_key(self.family(), owner)));
        ids.retain(|id| !symbols.is_member(id));
        ids
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
    indexes: &Indexes<'_>,
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

/// Rule 1: a qualified spelling. An import binding its first segment decides
/// first (rule 4, design 6.2 rule 3): an import of a module outside the graph
/// leaves the call a gap, and one whose files lack the rest hands it on. Then
/// the spelling against node scopes, longest first, then the qualifier as a
/// module path naming the files `name` is looked up in. A qualifier naming
/// nothing here is a call out of the graph and stays a gap, because every
/// same-named definition here is then a namesake rather than a candidate.
fn qualified(
    site: &Site,
    separator: &str,
    qualifier: &str,
    name: &str,
    keys: &mut BTreeSet<String>,
) -> Outcome {
    let segments: Vec<&str> = site.edge.symbol.split(separator).collect();
    match bindings::qualified_import(site, &segments, keys) {
        Step::Decided(outcome) => return outcome,
        Step::Refused => return Outcome::Unresolved,
        Step::Next => {}
    }
    if let Some(outcome) = scope_match(site, separator, &segments, keys) {
        return outcome;
    }
    let paths = &site.indexes.paths;
    let files = paths.module_files(qualifier, site.file, site.dialect, keys);
    if files.is_empty() {
        return Outcome::Unresolved;
    }
    let ids = site
        .symbols()
        .definitions_in_files(site.family(), name, &files, keys);
    site.decide(ids, EdgeProvenance::Import)
}

/// Rule 1's scope match: the longest suffix of the spelling that node scopes
/// end in, reached only by dropping leading segments that name something here.
/// `crate::a::Widget::new` drops `crate::a` to match `Widget::new`;
/// `std::io::Error::new` never drops `std::io` to reach a local `Error::new`.
/// An owner (the segment before the name) that only `impl` blocks carry is not
/// shown to be defined here: `String::from` beside `impl From<Name> for String`
/// may be std's, so the matches are candidates and nothing binds. `None` when
/// no such suffix matches.
fn scope_match(
    site: &Site,
    separator: &str,
    segments: &[&str],
    keys: &mut BTreeSet<String>,
) -> Option<Outcome> {
    let family = site.family();
    let symbols = site.symbols();
    for start in 0..segments.len() - 1 {
        let spelling = segments[start..].join("::");
        if symbols.lookup(family, &spelling, keys).is_empty()
            || !placed(site, separator, &segments[..start], keys)
        {
            continue;
        }
        let ids = symbols.definitions_in_family(family, &spelling, keys);
        let owner = segments[segments.len() - 2];
        if symbols.only_implemented(family, owner, keys) {
            return Some(site.candidates_only(ids));
        }
        return Some(site.decide(ids, EdgeProvenance::Import));
    }
    None
}

/// Whether the leading segments a qualified spelling drops name something in
/// this graph: none at all, a path starting at a root (Rust `crate`, `self`,
/// `super`, or the empty segment of C++ `::a::f`), or a module path the
/// dialect's conventions place on files.
fn placed(site: &Site, separator: &str, dropped: &[&str], keys: &mut BTreeSet<String>) -> bool {
    let Some(first) = dropped.first() else {
        return true;
    };
    if first.is_empty() || relative_spec(first) {
        return true;
    }
    let path = dropped.join(separator);
    let paths = &site.indexes.paths;
    let files = paths.module_files(&path, site.file, site.dialect, keys);
    !files.is_empty()
}

/// Rules 4 to 7 for a bare name: a named import, the package, the glob
/// imports, then graph-wide uniqueness. Rule 4's external module skips rule 6,
/// and either refusal leaves rule 7 recording candidates instead of binding.
/// Rules 5 to 7 see only definitions a bare name can reach
/// ([`Site::bare_reachable`]).
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
    let family = site.family();
    let found = site.symbols().definitions_in_family(family, name, keys);
    let ids = site.bare_reachable(found, keys);
    if refused {
        return site.candidates_only(ids);
    }
    site.decide(ids, EdgeProvenance::UniqueName)
}

/// Rule 5: `name` among the other files of the edge's package. `None` when the
/// family has no package scope or the package defines nothing by that name.
fn package_scope(site: &Site, name: &str, keys: &mut BTreeSet<String>) -> Option<Outcome> {
    let paths = &site.indexes.paths;
    let files = paths.package_files(site.file, site.dialect, keys);
    if files.is_empty() {
        return None;
    }
    let symbols = site.symbols();
    let found = symbols.definitions_in_files(site.family(), name, &files, keys);
    let ids = site.bare_reachable(found, keys);
    (!ids.is_empty()).then(|| site.decide(ids, EdgeProvenance::Import))
}
