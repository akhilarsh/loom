//! Java imports.
//!
//! `a.b.C` is `a/b/C.java`, suffix-matched because the source root is not known.
//! `a.b.*` is every `.java` file directly inside a directory `a/b`. A trailing
//! segment may name a member of the class (`a.b.C.helper`, `a.b.C.*` for a static
//! import), so shorter prefixes are tried, never below two segments. A glob import
//! may also arrive as the bare package `a.b`: when no class matches and a
//! directory `a/b` exists, that is the same package glob.

use super::{prefixes, PathIndex};

const FAMILY: &str = "java";

pub(super) fn module_files(paths: &PathIndex, spec: &str) -> Vec<String> {
    match spec.strip_suffix(".*") {
        Some(package) => glob(paths, &package.replace('.', "/")),
        None => class_or_package(paths, &spec.replace('.', "/")),
    }
}

/// The class a spec names, else every file in the package directory it spells: a
/// glob import is also recorded as the bare package (`a.b` with a glob flag), so
/// `a.b` is the mirror of `a.b.*` falling back to the class `a/b.java`.
fn class_or_package(paths: &PathIndex, path: &str) -> Vec<String> {
    let classes = class_files(paths, path);
    if classes.is_empty() {
        return paths.files_in_dirs_ending(path, FAMILY);
    }
    classes
}

/// Every file in the package directory, else the class whose members are
/// imported.
fn glob(paths: &PathIndex, package: &str) -> Vec<String> {
    let files = paths.files_in_dirs_ending(package, FAMILY);
    if files.is_empty() {
        return class_files(paths, package);
    }
    files
}

fn class_files(paths: &PathIndex, path: &str) -> Vec<String> {
    prefixes(path, 2)
        .map(|prefix| paths.first_suffix(FAMILY, &[format!("{prefix}.java")]))
        .find(|matched| !matched.is_empty())
        .unwrap_or_default()
}
