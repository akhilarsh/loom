//! PHP `use` declarations and `require`/`include` paths.
//!
//! `use A\B\C` follows the PSR-4 shape: `A/B/C.php`, suffix-matched, with leading
//! segments dropped because the autoload prefix (`App\` for `app/`) is not a
//! directory name in the graph. When no file matches, the spec may name a
//! namespace, which resolves through the namespace index with `\` written as `.`.
//!
//! A string path (`require 'lib/x.php'`, `include '../x.php'`) is a file path:
//! resolved against the including file's directory, then suffix-matched.

use std::collections::BTreeSet;

use super::{relative_to, PathIndex};

const FAMILY: &str = "php";

pub(super) fn module_files(
    paths: &PathIndex,
    spec: &str,
    from: &str,
    keys: &mut BTreeSet<String>,
) -> Vec<String> {
    let is_path = !spec.contains('\\') && (spec.contains('/') || spec.ends_with(".php"));
    if is_path {
        return included(paths, spec, from);
    }
    let name = spec.trim_start_matches('\\');
    let files = autoloaded(paths, &name.replace('\\', "/"));
    if !files.is_empty() {
        return files;
    }
    paths.namespace_files(FAMILY, &name.replace('\\', "."), keys)
}

/// `A/B/C` as `A/B/C.php`, then with leading segments dropped down to two.
fn autoloaded(paths: &PathIndex, path: &str) -> Vec<String> {
    let mut tail = path;
    loop {
        let found = paths.first_suffix(FAMILY, &[format!("{tail}.php")]);
        if !found.is_empty() {
            return found;
        }
        match tail.split_once('/') {
            Some((_, rest)) if rest.contains('/') => tail = rest,
            _ => return Vec::new(),
        }
    }
}

/// A file path, relative to the including file first.
fn included(paths: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    if let Some(base) = relative_to(from, spec) {
        let found = paths.first_exact(FAMILY, &[base]);
        if !found.is_empty() {
            return found;
        }
    }
    let plain = spec.trim_start_matches("./");
    paths.first_suffix(FAMILY, &[plain.to_string()])
}
