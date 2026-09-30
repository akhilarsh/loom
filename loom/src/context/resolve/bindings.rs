//! Rules that read the importing file's bindings: 2 (a member call on an
//! imported module or on a type a glob import brings into scope), 4 (a named or
//! aliased import) and 6 (glob imports).
//!
//! A binding's module spec goes through the dialect's path conventions. A spec
//! that matches no file is external — a dependency this graph does not hold —
//! unless it starts at a Rust anchor (`crate::`, `self::`, `super::`): such a
//! spec names a module of this project the conventions could not place
//! (`use super::*` in a `#[path]` test module), so it neither binds nor
//! refuses. `./x`, `../x` and Python's `.x` are placed exactly, so a miss is
//! external and refuses.
//!
//! A spec that spells the item it imports (C# `using S = App.Util.Strings;`)
//! and names no module of its own names the item's module instead: the files
//! of the spec with the item dropped that define the item.
//!
//! A name looked up in a module's files that none of them defines may be
//! re-exported: one of those files binds it with an import of its own (Python
//! `from .pricing import total as compute_total` in a package `__init__.py`),
//! and the lookup follows that import.

use std::collections::BTreeSet;
use std::path::Path;

use crate::context::extract::dialect::dialect_for_path;
use crate::context::graph_store::FileEntry;
use crate::context::source_graph::{EdgeProvenance, ImportBinding, SourceEdgeKind};

use super::rules::{Outcome, Site};
use super::symbols::declared_in;

/// How many re-exports one lookup follows. A longer chain is a cycle or a gap.
const MAX_REEXPORT_HOPS: usize = 4;

/// The family whose `References` edges are prototypes: the C and C++
/// extractors emit one for every function declared without a body.
const PROTOTYPE_FAMILY: &str = "c";

/// What a binding rule did with an edge.
pub(super) enum Step {
    /// The rule decided the edge.
    Decided(Outcome),
    /// The rule found the name bound to an external module: no later rule may
    /// bind it by name.
    Refused,
    /// The rule does not apply; the next one runs.
    Next,
}

/// Rule 2: a call on a receiver other than the enclosing type.
///
/// A receiver that is the local name of a non-glob import is a module or an
/// imported item: the member is looked up in the files the import names, and
/// exactly one hit binds with `Import`. A receiver naming a type that a glob
/// import brings into scope (`using App.Util;`, `import app.util.*;`) is that
/// type: `receiver::member` among the glob's files binds the same way. Any
/// other receiver is a value whose type is unknown here, so it is never bound:
/// definitions named like the member become candidates.
pub(super) fn member_call(site: &Site, receiver: &str, keys: &mut BTreeSet<String>) -> Outcome {
    let member = site.edge.symbol.as_str();
    if let Some(binding) = bound_as(site, receiver) {
        return through(site, binding, member, keys);
    }
    glob_member(site, receiver, member, keys).unwrap_or_else(|| site.name_candidates(member, keys))
}

/// Rule 4 for a qualified spelling whose first segment is an import's local
/// name: the rest of the spelling is looked up in the files the import names.
/// `None` when no import binds that segment.
pub(super) fn qualified_import(
    site: &Site,
    segments: &[&str],
    keys: &mut BTreeSet<String>,
) -> Option<Outcome> {
    let (first, rest) = segments.split_first()?;
    let binding = bound_as(site, first)?;
    Some(through(site, binding, &rest.join("::"), keys))
}

/// Rule 4 for a bare name an import binds: the imported name (`parse` for
/// `import { parse as p }`) is looked up in the files the import names.
pub(super) fn named_import(site: &Site, name: &str, keys: &mut BTreeSet<String>) -> Step {
    let Some(binding) = bound_as(site, name) else {
        return Step::Next;
    };
    let files = module_files(site, binding, site.file, keys);
    if files.is_empty() {
        // A Rust anchor the conventions cannot place is still this project's.
        return if relative_spec(&binding.path) {
            Step::Next
        } else {
            Step::Refused
        };
    }
    let target = binding.name.as_deref().unwrap_or(name);
    let ids = exported(site, &files, target, MAX_REEXPORT_HOPS, keys);
    if ids.is_empty() {
        return Step::Next;
    }
    Step::Decided(site.decide(ids, EdgeProvenance::Import))
}

/// Rule 6: `name` in the files of every glob import that resolves. One hit
/// binds with `Import`; a glob import naming an external module refuses rule
/// 7 when nothing was found, unless a prototype declares the name here.
pub(super) fn globs(site: &Site, name: &str, keys: &mut BTreeSet<String>) -> Step {
    let mut hits = BTreeSet::new();
    let mut placed = Vec::new();
    let mut refused = false;
    for binding in site.entry.imports.iter().filter(|binding| binding.glob) {
        let files = module_files(site, binding, site.file, keys);
        if files.is_empty() {
            // Only an unplaceable Rust anchor is exempt from refusing.
            refused |= !relative_spec(&binding.path);
            continue;
        }
        let symbols = site.symbols();
        hits.extend(symbols.definitions_in_files(site.family(), name, &files, keys));
        placed.extend(files);
    }
    if !hits.is_empty() {
        let ids = hits.into_iter().collect();
        return Step::Decided(site.decide(ids, EdgeProvenance::Import));
    }
    if refused && !prototyped(site, name, &placed, keys) {
        Step::Refused
    } else {
        Step::Next
    }
}

/// Rule 2 for a receiver no import binds: `receiver::member` among the files
/// of the edge's glob imports. `None` when none of them defines it.
fn glob_member(
    site: &Site,
    receiver: &str,
    member: &str,
    keys: &mut BTreeSet<String>,
) -> Option<Outcome> {
    let spelling = format!("{receiver}::{member}");
    let mut hits = BTreeSet::new();
    for binding in site.entry.imports.iter().filter(|binding| binding.glob) {
        let files = module_files(site, binding, site.file, keys);
        let symbols = site.symbols();
        hits.extend(symbols.definitions_in_files(site.family(), &spelling, &files, keys));
    }
    let ids: Vec<String> = hits.into_iter().collect();
    (!ids.is_empty()).then(|| site.decide(ids, EdgeProvenance::Import))
}

/// Whether a prototype declares `name` in the edge's own file or in one of the
/// included `headers` that resolved. A C or C++ function declared by the
/// project itself is the project's: a system header cannot be its source, and
/// the linker binds the one external definition, so rule 7 may decide it.
fn prototyped(site: &Site, name: &str, headers: &[String], keys: &mut BTreeSet<String>) -> bool {
    if site.family() != PROTOTYPE_FAMILY {
        return false;
    }
    let declares = |entry: &FileEntry| {
        entry
            .edges
            .iter()
            .any(|edge| edge.kind == SourceEdgeKind::References && edge.symbol == name)
    };
    declares(site.entry)
        || headers
            .iter()
            .any(|header| site.indexes.entry(header, keys).is_some_and(declares))
}

/// `member` in the files `binding` names. An imported item's members are
/// spelled `item::member`; a binding may also name a module (Rust's
/// `use a::b;`), whose members are its own definitions or its re-exports, so
/// the bare member is tried second. Nothing found, or an external module,
/// stays a gap: the receiver or qualifier said where the callee lives.
fn through(
    site: &Site,
    binding: &ImportBinding,
    member: &str,
    keys: &mut BTreeSet<String>,
) -> Outcome {
    let files = module_files(site, binding, site.file, keys);
    if files.is_empty() {
        return Outcome::Unresolved;
    }
    if let Some(item) = binding.name.as_deref() {
        let spelling = format!("{item}::{member}");
        let symbols = site.symbols();
        let ids = symbols.definitions_in_files(site.family(), &spelling, &files, keys);
        if !ids.is_empty() {
            return site.decide(ids, EdgeProvenance::Import);
        }
    }
    let ids = exported(site, &files, member, MAX_REEXPORT_HOPS, keys);
    if ids.is_empty() {
        return Outcome::Unresolved;
    }
    site.decide(ids, EdgeProvenance::Import)
}

/// Definitions of `name` in `files`. When none of them defines it, the
/// non-glob imports those files bind `name` with are followed, up to `hops`
/// deep: what such an import names, in the files it names from the file that
/// wrote it, is what the module exports under `name`.
fn exported(
    site: &Site,
    files: &[String],
    name: &str,
    hops: usize,
    keys: &mut BTreeSet<String>,
) -> Vec<String> {
    let ids = site
        .symbols()
        .definitions_in_files(site.family(), name, files, keys);
    if !ids.is_empty() || hops == 0 {
        return ids;
    }
    let mut found = BTreeSet::new();
    for file in files {
        let Some(entry) = site.indexes.entry(file, keys) else {
            continue;
        };
        let reexports = entry.imports.iter();
        for binding in reexports.filter(|binding| binding.local_name() == Some(name)) {
            let next = module_files(site, binding, file, keys);
            let target = binding.name.as_deref().unwrap_or(name);
            found.extend(exported(site, &next, target, hops - 1, keys));
        }
    }
    found.into_iter().collect()
}

/// The non-glob import of the edge's file that binds `local`.
fn bound_as<'a>(site: &Site<'a>, local: &str) -> Option<&'a ImportBinding> {
    site.entry
        .imports
        .iter()
        .find(|binding| binding.local_name() == Some(local))
}

/// The files `binding`'s module spec names from the file at `from`, falling
/// back to the files of the item's module that define the item when the spec
/// spells an item and names no module itself.
fn module_files(
    site: &Site,
    binding: &ImportBinding,
    from: &str,
    keys: &mut BTreeSet<String>,
) -> Vec<String> {
    let Some(dialect) = dialect_for_path(Path::new(from)) else {
        return Vec::new();
    };
    let paths = &site.indexes.paths;
    let files = paths.module_files(&binding.path, from, dialect, keys);
    if !files.is_empty() {
        return files;
    }
    let (Some(item), Some(parent)) = (binding.name.as_deref(), item_module(binding)) else {
        return files;
    };
    let module = paths.module_files(parent, from, dialect, keys);
    let symbols = site.symbols();
    let defining = symbols.definitions_in_files(site.family(), item, &module, keys);
    module
        .into_iter()
        .filter(|file| defining.iter().any(|id| declared_in(id, file)))
        .collect()
}

/// The spec of a non-glob `binding` with its imported item's last segment
/// and separator dropped: `App.Util` for `App.Util.Strings` importing
/// `Strings`. `None` when the spec does not end in a separated item.
fn item_module(binding: &ImportBinding) -> Option<&str> {
    let item = binding.name.as_deref().filter(|_| !binding.glob)?;
    let head = binding.path.strip_suffix(item)?;
    let parent = head.trim_end_matches(['.', ':', '\\', '/']);
    (!parent.is_empty() && parent.len() < head.len()).then_some(parent)
}

/// Whether `spec` starts at a Rust anchor (`crate`, `self`, `super`). Such a path
/// names a module of this project that the conventions cannot always place
/// (`use super::*` in a `#[path]` test module). Path-style specs (`./x`,
/// `../x`, Python's `.x`) are placed exactly by the conventions, so one that
/// matches no file is external.
fn relative_spec(spec: &str) -> bool {
    let root = spec.split("::").next().unwrap_or(spec);
    matches!(root, "crate" | "self" | "super")
}
