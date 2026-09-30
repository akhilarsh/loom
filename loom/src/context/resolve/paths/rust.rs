//! Rust module paths.
//!
//! Three rules, in order. A path that says where it starts from — `crate::`,
//! `self::`, `super::` — is **anchored**: it is joined onto a crate root or onto
//! the module the citing file belongs to, and only files under that anchor can
//! match. When nothing sits under the anchor, the path is suffix-matched with the
//! anchor keyword dropped, which is all a graph with no crate root can offer.
//!
//! Any other path (`x::y`) is internal only when its first segment names a module
//! file (`x.rs`, `x/mod.rs`) or directory under a crate root or under the citing
//! module's directory. Otherwise the import is external: `use serde::de::*` is
//! never the `de.rs` of this project.
//!
//! One exception to that: an integration test, bench or example imports its own
//! crate by Cargo package name (`use demo::add;` in `tests/add_test.rs`), a name
//! the graph does not hold. When the citing file lies outside every crate-root
//! directory and the first segment names nothing local, that segment is dropped
//! and the rest resolves as `crate::<rest>` would, against the crate roots whose
//! package directory (the parent of the crate root directory) is an ancestor of
//! the citing file. Inside a crate root directory the rule never applies.
//!
//! Anchoring is what separates two files of the same name: `crate::codex` is
//! `<crate root>/codex.rs` and never the `codex.rs` sitting four directories
//! deeper. A relative anchor is weaker than a rooted one and is used as such:
//! `self::` and `super::` reach files *below* the module they name but never that
//! module itself, because the same line means different modules at the top of a
//! file and inside an inline `mod` block, and extraction does not record which.

use super::{directory_of, join, last_segment, prefixes, PathIndex};

const FAMILY: &str = "rust";

/// Spellings tried after the bare module path, in order. The first form matching
/// anything decides; later forms are never consulted.
///
/// The `/lib.rs` and `/main.rs` forms are what let a directory match as a crate
/// root, so an item written `crate::x` and defined in `lib.rs` itself is
/// reachable; they stay last so a module file is always preferred.
const MODULE_SUFFIXES: [&str; 4] = [".rs", "/mod.rs", "/lib.rs", "/main.rs"];

/// Path roots that say where the path starts from.
const ANCHORS: [&str; 3] = ["crate", "self", "super"];

/// Files that are the module they sit in rather than a module below it, so a
/// path written inside them starts from their own directory.
const MODULE_ROOT_FILES: [&str; 3] = ["mod.rs", "lib.rs", "main.rs"];

pub(super) fn module_files(paths: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    let written = written_path(spec);
    let (root, rest) = written.split_once("::").unwrap_or((written, ""));
    if !ANCHORS.contains(&root) {
        let local = unanchored(paths, &to_path(written), from);
        if local.is_empty() {
            return package_named(paths, &to_path(rest), from);
        }
        return local;
    }
    let anchored = anchored(paths, root, &to_path(rest), from);
    if !anchored.is_empty() {
        return anchored;
    }
    suffix_candidates(paths, &to_path(rest))
}

/// The path part of an import as written: everything before a brace group, a
/// glob, or an `as` clause, none of which name a file. A `use` path can arrive
/// wrapped across lines, so whitespace ends the path too.
fn written_path(spec: &str) -> &str {
    let end = spec
        .find(['{', '*', ' ', '\t', '\n', '\r'])
        .unwrap_or(spec.len());
    spec[..end].trim_end_matches(':')
}

fn to_path(written: &str) -> String {
    written.replace("::", "/")
}

/// Candidates for a path that names where it starts from.
///
/// `crate::` is tried under every crate root in the graph — more than one means
/// more than one crate is indexed, and the uniqueness rule settles it — and may
/// land on the root's own `lib.rs`, since an item re-exported there is still
/// written `crate::x`.
///
/// `self::` and `super::` are tried strictly *below* the module the citing file
/// belongs to, and never resolve to that module itself.
fn anchored(paths: &PathIndex, root: &str, relative: &str, from: &str) -> Vec<String> {
    let mut matched: Vec<String> = match root {
        "crate" => paths
            .crate_roots
            .iter()
            .flat_map(|anchor| under_or_anchor(paths, anchor, relative))
            .collect(),
        "self" => under(paths, &module_dir(from), relative),
        _ => parent_module(from)
            .map(|anchor| under(paths, &anchor, relative))
            .unwrap_or_default(),
    };
    matched.sort();
    matched.dedup();
    matched
}

/// Candidates for a path whose first segment is not an anchor keyword: the path
/// read from each crate root and from the citing module's own directory. Each
/// probe is exact, so a first segment that is no module file or directory under
/// either anchor — an external crate — matches nothing.
fn unanchored(paths: &PathIndex, relative: &str, from: &str) -> Vec<String> {
    let mut anchors = paths.crate_roots.clone();
    anchors.insert(module_dir(from));
    let mut matched: Vec<String> = anchors
        .iter()
        .flat_map(|anchor| under(paths, anchor, relative))
        .collect();
    matched.sort();
    matched.dedup();
    matched
}

/// Candidates for a path whose first segment names nothing local and is dropped
/// (`relative` is what follows it): the Cargo package name an integration test,
/// bench or example uses to import its own crate. The package name is not in the graph, so the rule is
/// positional. It applies only to a citing file outside every crate-root
/// directory, and reads the rest of the path as `crate::<rest>` against the crate
/// roots whose package directory (the root directory's parent) is an ancestor of
/// the citing file. A file inside a crate root never qualifies, so
/// `use serde::de::*` from `src/x.rs` stays external.
fn package_named(paths: &PathIndex, relative: &str, from: &str) -> Vec<String> {
    let inside_a_crate = paths.crate_roots.iter().any(|root| is_within(from, root));
    if relative.is_empty() || inside_a_crate {
        return Vec::new();
    }
    let mut matched: Vec<String> = paths
        .crate_roots
        .iter()
        .filter(|root| is_within(from, directory_of(root)))
        .flat_map(|root| under_or_anchor(paths, root, relative))
        .collect();
    matched.sort();
    matched.dedup();
    matched
}

/// Whether `file` lies in `dir` or below it, compared on path components. The
/// empty directory is the repository root and holds every file.
fn is_within(file: &str, dir: &str) -> bool {
    dir.is_empty()
        || file
            .strip_prefix(dir)
            .is_some_and(|tail| tail.starts_with('/'))
}

/// Candidates for `relative` under `anchor`, dropping trailing segments — which
/// name items rather than files — until something matches. The anchor itself is
/// never shortened and never matched, so an anchored path can only ever reach a
/// file *below* the module it named.
fn under(paths: &PathIndex, anchor: &str, relative: &str) -> Vec<String> {
    prefixes(relative, 1)
        .filter(|prefix| !prefix.is_empty())
        .map(|prefix| paths.first_exact(FAMILY, &spellings(&join(anchor, prefix))))
        .find(|matched| !matched.is_empty())
        .unwrap_or_default()
}

/// [`under`], falling back to the anchor's own module file. Only for an anchor
/// that one written path can mean, which is a crate root and nothing else.
fn under_or_anchor(paths: &PathIndex, anchor: &str, relative: &str) -> Vec<String> {
    let below = under(paths, anchor, relative);
    if below.is_empty() {
        return paths.first_exact(FAMILY, &spellings(anchor));
    }
    below
}

/// File node ids matched by a path with its anchor keyword dropped, suffix
/// matched against every Rust file.
///
/// The last segment of a `use` path is usually the *item*, not the file:
/// `crate::context::graph_store::GraphStore` names a type inside
/// `context/graph_store.rs`. So when no spelling of the full path matches, the
/// trailing segment is dropped and the spellings are tried again, down to a
/// single segment. Truncation only ever widens the candidate set, and the
/// uniqueness rule still decides: an over-short prefix like `mod` matches many
/// files and therefore resolves nothing.
fn suffix_candidates(paths: &PathIndex, relative: &str) -> Vec<String> {
    prefixes(relative, 1)
        .filter(|prefix| !prefix.is_empty())
        .map(|prefix| paths.first_suffix(FAMILY, &spellings(prefix)))
        .find(|matched| !matched.is_empty())
        .unwrap_or_default()
}

/// Every spelling a module path `base` can take as a file.
fn spellings(base: &str) -> Vec<String> {
    if base.is_empty() {
        return Vec::new();
    }
    MODULE_SUFFIXES
        .iter()
        .map(|suffix| format!("{base}{suffix}"))
        .collect()
}

/// The directory a module path written inside `file` starts from: the file's own
/// directory when it is a module root, and the directory the file names
/// otherwise — `src/a.rs` is module `a`, whose children live in `src/a/`.
fn module_dir(file: &str) -> String {
    let name = last_segment(file);
    if MODULE_ROOT_FILES.contains(&name) {
        return directory_of(file).to_string();
    }
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    join(directory_of(file), stem)
}

/// The directory one module above `file`, or `None` at the top of the tree.
fn parent_module(file: &str) -> Option<String> {
    module_dir(file)
        .rsplit_once('/')
        .map(|(head, _)| head.to_string())
}
