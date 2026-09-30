//! Which nodes a written name could mean, within one resolution family.
//!
//! A node is indexed under its bare name and under every scope-qualified
//! suffix of it, so a call written `Widget::new()` can be matched against the
//! `new` inside `impl Widget` without the bare `new` — a name dozens of types
//! share — deciding anything. Buckets are keyed by family first: a Python
//! call never sees a Go definition.
//!
//! Every lookup inserts its key into the set the caller passes, which is how a
//! resolution records what it consulted (see `record`).

use std::collections::{BTreeMap, BTreeSet};

use crate::context::graph_store::ResolvedGraph;
use crate::context::source_graph::SourceNodeKind;

use super::record::{answers_to, name_key};

/// Family -> spelling -> ids of the nodes answering to that spelling.
type Buckets = BTreeMap<String, BTreeMap<String, Vec<String>>>;

/// `(family, spelling)` -> ids of the nodes defining that spelling.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    by_name: Buckets,
    /// Ids of [`SourceNodeKind::Implementation`] nodes, which are indexed by
    /// name but do not define one. See [`SymbolIndex::definitions_in_family`].
    implementations: BTreeSet<String>,
}

impl SymbolIndex {
    /// Index every node under the spelling it answers to, in the family of its
    /// file. Nodes of a file no dialect claims are not indexed.
    pub fn build(graph: &ResolvedGraph) -> Self {
        let mut index = SymbolIndex::default();
        for node in graph.nodes() {
            let Some((family, spellings)) = answers_to(node) else {
                continue;
            };
            let bucket = index.by_name.entry(family.to_string()).or_default();
            for spelling in spellings {
                bucket.entry(spelling).or_default().push(node.id.clone());
            }
            if node.kind == SourceNodeKind::Implementation {
                index.implementations.insert(node.id.clone());
            }
        }
        // Sorted and deduplicated, so a candidate list is always ordered before
        // its length or first element is read. Determinism depends on it.
        for ids in index.by_name.values_mut().flat_map(BTreeMap::values_mut) {
            ids.sort();
            ids.dedup();
        }
        index
    }

    /// Ids of every node answering to `spelling` in `family`, `impl` blocks
    /// included, sorted. Empty when unknown.
    pub fn lookup(&self, family: &str, spelling: &str, keys: &mut BTreeSet<String>) -> &[String] {
        const UNKNOWN: &[String] = &[];
        keys.insert(name_key(family, spelling));
        self.by_name
            .get(family)
            .and_then(|bucket| bucket.get(spelling))
            .map_or(UNKNOWN, Vec::as_slice)
    }

    /// Ids defining `name` in `family`, with `impl`-block nodes removed.
    ///
    /// An `impl` block is scoped under the bare type name, so it lands in the
    /// same bucket as the type — but it does not *define* that name, it attaches
    /// to a type already indexed under it. Counting it as a rival would make
    /// every type that has an `impl` permanently ambiguous, so this is the one
    /// place the ambiguity rule is deliberately narrowed, and it is narrowed for
    /// [`SourceNodeKind::Implementation`] alone: two functions sharing a name are
    /// still genuinely contested, and a name whose only candidates are `impl`
    /// blocks resolves to nothing.
    pub(super) fn definitions_in_family(
        &self,
        family: &str,
        name: &str,
        keys: &mut BTreeSet<String>,
    ) -> Vec<String> {
        self.lookup(family, name, keys)
            .iter()
            .filter(|id| !self.implementations.contains(id.as_str()))
            .cloned()
            .collect()
    }

    /// Definitions of `name` whose scope ends in `type_scope::name`: the members
    /// of a type, wherever the type's parts are declared.
    pub(super) fn members(
        &self,
        family: &str,
        type_scope: &str,
        name: &str,
        keys: &mut BTreeSet<String>,
    ) -> Vec<String> {
        self.definitions_in_family(family, &format!("{type_scope}::{name}"), keys)
    }

    /// Ids defining `name` inside one of `files`, for a call whose path, import
    /// or package already named the module the callee lives in. No files means
    /// no candidates.
    pub(super) fn definitions_in_files(
        &self,
        family: &str,
        name: &str,
        files: &[String],
        keys: &mut BTreeSet<String>,
    ) -> Vec<String> {
        self.definitions_in_family(family, name, keys)
            .into_iter()
            .filter(|id| files.iter().any(|file| declared_in(id, file)))
            .collect()
    }

    /// Every `(family, spelling, ids)` bucket of the index.
    #[cfg(test)]
    pub(super) fn buckets(&self) -> impl Iterator<Item = (&str, &str, &[String])> {
        self.by_name.iter().flat_map(|(family, bucket)| {
            bucket
                .iter()
                .map(move |(spelling, ids)| (family.as_str(), spelling.as_str(), ids.as_slice()))
        })
    }
}

/// Whether a node id belongs to `file`. Ids are `<path>#<kind>:<scope>`, so the
/// separator has to be checked or `src/a.rs` would claim `src/a.rs.bak`.
fn declared_in(id: &str, file: &str) -> bool {
    id.strip_prefix(file)
        .is_some_and(|rest| rest.starts_with('#'))
}
