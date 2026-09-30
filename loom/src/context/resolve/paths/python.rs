//! Python module specs: dotted absolute names and leading-dot relative forms.
//!
//! `a.b` is `a/b.py` or `a/b/__init__.py`, suffix-matched because the package
//! root is not known. `.x` and `..x` are resolved against the importing file's
//! package directory, one level up per extra dot, and probed exactly.

use super::{directory_of, join, prefixes, PathIndex};

const FAMILY: &str = "python";

/// The forms a module path can take as a file, tried in order.
const MODULE_SUFFIXES: [&str; 2] = [".py", "/__init__.py"];

pub(super) fn module_files(paths: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    if spec.starts_with('.') {
        relative(paths, spec, from)
    } else {
        absolute(paths, spec)
    }
}

/// A dotted name. The last segment of `from a.b import c` may be an item rather
/// than a module, so shorter prefixes are tried, but never below two segments:
/// a lone leading package name is too loose to suffix-match.
///
/// A spec written as one segment (`import utils`) is still suffix-matched
/// whole, anywhere in the graph. That is a trade-off: the package root is not
/// known, so a project's top-level module can sit under `src/` or at the root
/// and only a suffix finds it; the cost is that a standard-library or
/// third-party module sharing a project file's name (`json`, `logging`) is
/// taken for that file. Ruby's bare `require 'x'` makes the same trade; Go does
/// not, because a module-local Go import always carries the module path.
fn absolute(paths: &PathIndex, spec: &str) -> Vec<String> {
    let path = spec.replace('.', "/");
    let found = prefixes(&path, 2)
        .map(|prefix| paths.first_suffix(FAMILY, &spellings(prefix)))
        .find(|matched| !matched.is_empty());
    found.unwrap_or_default()
}

/// A leading-dot name, anchored on the importing file's package directory.
fn relative(paths: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    let dots = spec.chars().take_while(|c| *c == '.').count();
    let Some(base) = ascend(directory_of(from), dots - 1) else {
        return Vec::new();
    };
    let rest = spec[dots..].replace('.', "/");
    if rest.is_empty() {
        return paths.first_exact(FAMILY, &[join(&base, "__init__.py")]);
    }
    let found = prefixes(&rest, 1)
        .map(|prefix| paths.first_exact(FAMILY, &spellings(&join(&base, prefix))))
        .find(|matched| !matched.is_empty());
    found.unwrap_or_default()
}

/// `directory` climbed `levels` directories, or `None` above the root.
fn ascend(directory: &str, levels: usize) -> Option<String> {
    let mut current = directory;
    for _ in 0..levels {
        if current.is_empty() {
            return None;
        }
        current = directory_of(current);
    }
    Some(current.to_string())
}

fn spellings(base: &str) -> Vec<String> {
    MODULE_SUFFIXES
        .iter()
        .map(|suffix| format!("{base}{suffix}"))
        .collect()
}
