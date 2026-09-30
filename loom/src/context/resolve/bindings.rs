//! Rules that read the importing file's bindings: 2 (a member call on an
//! imported module), 4 (a named or aliased import) and 6 (glob imports).
//!
//! A binding's module spec goes through the dialect's path conventions. A spec
//! that matches no file is external — a dependency this graph does not hold —
//! unless it starts at a Rust anchor (`crate::`, `self::`, `super::`): such a
//! spec names a module of this project the conventions could not place
//! (`use super::*` in a `#[path]` test module), so it neither binds nor
//! refuses. `./x`, `../x` and Python's `.x` are placed exactly, so a miss is
//! external and refuses.

use std::collections::BTreeSet;

use crate::context::source_graph::{EdgeProvenance, ImportBinding};

use super::rules::{Outcome, Site};

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
/// exactly one hit binds with `Import`. Any other receiver is a value whose
/// type is unknown here, so it is never bound: definitions named like the
/// member become candidates.
pub(super) fn member_call(site: &Site, receiver: &str, keys: &mut BTreeSet<String>) -> Outcome {
    let member = site.edge.symbol.as_str();
    match bound_as(site, receiver) {
        Some(binding) => through(site, binding, member, keys),
        None => site.name_candidates(member, keys),
    }
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
    let files = module_files(site, binding, keys);
    if files.is_empty() {
        // A Rust anchor the conventions cannot place is still this project's.
        return if relative_spec(&binding.path) {
            Step::Next
        } else {
            Step::Refused
        };
    }
    let target = binding.name.as_deref().unwrap_or(name);
    let ids = site
        .symbols()
        .definitions_in_files(site.family(), target, &files, keys);
    if ids.is_empty() {
        return Step::Next;
    }
    Step::Decided(site.decide(ids, EdgeProvenance::Import))
}

/// Rule 6: `name` in the files of every glob import that resolves. One hit
/// binds with `Import`; a glob import naming an external module refuses rule
/// 7 when nothing was found.
pub(super) fn globs(site: &Site, name: &str, keys: &mut BTreeSet<String>) -> Step {
    let mut hits = BTreeSet::new();
    let mut refused = false;
    for binding in site.entry.imports.iter().filter(|binding| binding.glob) {
        let files = module_files(site, binding, keys);
        if files.is_empty() {
            // Only an unplaceable Rust anchor is exempt from refusing.
            refused |= !relative_spec(&binding.path);
            continue;
        }
        let symbols = site.symbols();
        hits.extend(symbols.definitions_in_files(site.family(), name, &files, keys));
    }
    if !hits.is_empty() {
        let ids = hits.into_iter().collect();
        return Step::Decided(site.decide(ids, EdgeProvenance::Import));
    }
    if refused {
        Step::Refused
    } else {
        Step::Next
    }
}

/// `member` in the files `binding` names. An imported item's members are
/// spelled `item::member`; a binding may also name a module (Rust's
/// `use a::b;`), whose members are its own definitions, so the bare member is
/// tried second. Nothing found, or an external module, stays a gap: the
/// receiver or qualifier said where the callee lives.
fn through(
    site: &Site,
    binding: &ImportBinding,
    member: &str,
    keys: &mut BTreeSet<String>,
) -> Outcome {
    let files = module_files(site, binding, keys);
    if files.is_empty() {
        return Outcome::Unresolved;
    }
    let qualified = binding
        .name
        .as_deref()
        .map(|item| format!("{item}::{member}"));
    for spelling in qualified.iter().map(String::as_str).chain([member]) {
        let symbols = site.symbols();
        let ids = symbols.definitions_in_files(site.family(), spelling, &files, keys);
        if !ids.is_empty() {
            return site.decide(ids, EdgeProvenance::Import);
        }
    }
    Outcome::Unresolved
}

/// The non-glob import of the edge's file that binds `local`.
fn bound_as<'a>(site: &Site<'a>, local: &str) -> Option<&'a ImportBinding> {
    site.entry
        .imports
        .iter()
        .find(|binding| binding.local_name() == Some(local))
}

/// The files `binding`'s module spec names from the edge's file.
fn module_files(site: &Site, binding: &ImportBinding, keys: &mut BTreeSet<String>) -> Vec<String> {
    let paths = &site.indexes.paths;
    paths.module_files(&binding.path, site.file, site.dialect, keys)
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
