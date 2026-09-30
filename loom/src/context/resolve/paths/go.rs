//! Go import paths.
//!
//! An import path names a package, and a package is a directory: every `.go` file
//! directly inside the matched directory is a candidate. The module path prefix
//! (`example.com/m/`) is not a directory in the graph, so the path is matched as a
//! suffix of a directory, and when the path starts with a host name, leading
//! segments are dropped until a directory matches. A path with no host is only
//! ever matched whole as a directory suffix (`internal/http`). Module-local
//! imports always carry the module path, so a one-segment path without a host
//! (`fmt`, `errors`) is the standard library and matches no file: it cannot land
//! on a local directory of the same name.

use super::PathIndex;

const FAMILY: &str = "go";

pub(super) fn module_files(paths: &PathIndex, spec: &str) -> Vec<String> {
    if !spec.contains('/') {
        return Vec::new();
    }
    tails(spec)
        .into_iter()
        .map(|tail| paths.files_in_dirs_ending(tail, FAMILY))
        .find(|matched| !matched.is_empty())
        .unwrap_or_default()
}

/// The path, then, for a path starting with a host, each suffix of it.
fn tails(spec: &str) -> Vec<&str> {
    let mut tails = vec![spec];
    let hosted = spec
        .split('/')
        .next()
        .is_some_and(|first| first.contains('.'));
    if hosted {
        let mut rest = spec;
        while let Some((_, tail)) = rest.split_once('/') {
            tails.push(tail);
            rest = tail;
        }
    }
    tails.retain(|tail| !tail.is_empty());
    tails
}
