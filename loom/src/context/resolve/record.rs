//! What resolving an edge consulted, and what a file's nodes answer to.
//!
//! Every lookup a resolution rule makes is recorded as a key, so an incremental
//! relink can find the edges a changed file may resolve differently:
//!
//! - `name:{family}:{name}` for a definition-name lookup, where `{name}` is the
//!   last `::` or `.` segment of the spelling looked up;
//! - `ns:{family}:{namespace}` for a namespace-index lookup;
//! - `pathset:{family}` for a module-path or package-scope lookup, which depends
//!   only on which files exist.
//!
//! A rule that reads another file's own entry (a re-export among its import
//! bindings, a prototype among its edges) records that file's `name:` keys for
//! its file node ([`record_file`]): every change to the file touches them.
//!
//! [`answers_to`] is the one definition of the spellings a node is indexed
//! under. [`SymbolIndex::build`](super::SymbolIndex::build) indexes by it and
//! [`touched_keys`] emits the keys of it, so every bucket a lookup can land in
//! maps to a key that the file owning the bucket's nodes touches.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::context::graph_store::FileEntry;
use crate::context::source_graph::{SourceNode, SourceNodeKind};

use super::paths::{family_of, ns_key, pathset_key};

/// One edge of a graph: the file it was extracted from and its index into that
/// entry's `edges`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeRef {
    pub path: String,
    pub index: usize,
}

/// The lookup keys each resolved edge consulted.
pub type EdgeKeys = BTreeMap<EdgeRef, BTreeSet<String>>;

/// The key recorded for looking up `spelling` among the definitions of
/// `family`: `Widget::new` and `new` share one key, because a node answering to
/// either is named `new`.
pub(super) fn name_key(family: &str, spelling: &str) -> String {
    let name = spelling.rsplit([':', '.']).next().unwrap_or(spelling);
    format!("name:{family}:{name}")
}

/// The family `node` resolves in and every spelling it answers to: its
/// [`node_names`] plus every scope-qualified suffix. `None` for a node of a
/// file no dialect claims, which no resolution rule can reach.
pub(super) fn answers_to(node: &SourceNode) -> Option<(&'static str, Vec<String>)> {
    let family = family_of(&node.path)?;
    let mut spellings = node_names(node);
    spellings.extend(qualified_names(node));
    Some((family, spellings))
}

/// Every key a change to `entry` can invalidate: the `name:` key of every
/// spelling its nodes answer to, the `ns:` key of every namespace or package
/// its `Module` nodes declare, and, when the file was added or removed, the
/// `pathset:` key of its family.
pub fn touched_keys(entry: &FileEntry, added_or_removed: bool) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for node in &entry.nodes {
        let Some((family, spellings)) = answers_to(node) else {
            continue;
        };
        keys.extend(spellings.iter().map(|spelling| name_key(family, spelling)));
        if node.kind == SourceNodeKind::Module && !node.scope.is_empty() {
            keys.insert(ns_key(family, &node.scope.join(".")));
        }
        if added_or_removed {
            keys.insert(pathset_key(family));
        }
    }
    keys
}

/// Record that a rule read the entry of the file at `path` itself (its import
/// bindings or its edges), not only its definitions: the keys its file node
/// answers to, which [`touched_keys`] emits for every change to that file.
pub(super) fn record_file(path: &str, keys: &mut BTreeSet<String>) {
    let Some(family) = family_of(path) else {
        return;
    };
    for name in file_names(Path::new(path)) {
        keys.insert(name_key(family, &name));
    }
}

/// Every name a node is indexed under. A file claims both its file name and its
/// extension-less stem, so `language` finds `src/language.rs`; a stem that also
/// names a symbol shares a bucket precisely so resolution refuses to pick.
pub(crate) fn node_names(node: &SourceNode) -> Vec<String> {
    if node.kind == SourceNodeKind::File {
        return file_names(&node.path);
    }
    node.scope.last().cloned().into_iter().collect()
}

/// A file's name and its extension-less stem.
fn file_names(path: &Path) -> Vec<String> {
    [path.file_name(), path.file_stem()]
        .into_iter()
        .flatten()
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
}

/// Every scope-qualified spelling a node also answers to: a `helper` in
/// `impl Widget` inside `mod example` is `Widget::helper` and
/// `example::Widget::helper`. The one-segment spelling is [`node_names`]'s job,
/// and a file node has no scope to qualify.
///
/// This is the whole-graph twin of the same-file lookup the extractor builds in
/// `extract::treesitter::build::spellings`; the two must agree on what a scope
/// is spelled as, or one pass would resolve a call the other could not see.
fn qualified_names(node: &SourceNode) -> Vec<String> {
    (0..node.scope.len().saturating_sub(1))
        .map(|start| node.scope[start..].join("::"))
        .collect()
}
