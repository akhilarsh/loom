//! The bindings one Python `import` or `from ... import` statement declares.

use crate::context::source_graph::{ImportBinding, Span};

/// The bindings of `statement` for its module `path`, all at `site`.
///
/// A `from` statement yields one binding per imported name. An `import`
/// statement is matched once per module it names, so it yields only the
/// binding of `path`.
pub(super) fn statement_bindings(statement: &str, path: &str, site: Span) -> Vec<ImportBinding> {
    let text = compact(statement);
    if text.starts_with("from") {
        return from_bindings(&text, path, site);
    }
    let Some(items) = text.strip_prefix("import") else {
        return Vec::new();
    };
    items
        .split(',')
        .map(str::trim)
        .filter_map(|item| {
            let (module, alias) = match item.split_once(" as ") {
                Some((module, alias)) => (module.trim(), Some(alias.trim().to_string())),
                None => (item, None),
            };
            (module == path).then(|| ImportBinding {
                path: path.to_string(),
                name: None,
                alias: alias.or_else(|| dotted(path)),
                glob: false,
                exported_as: None,
                site,
            })
        })
        .collect()
}

/// One binding per name in `from <path> import <names>`: `*` is a glob.
fn from_bindings(text: &str, path: &str, site: Span) -> Vec<ImportBinding> {
    let Some(names) = imported_names(text, path) else {
        return Vec::new();
    };
    names
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            let (name, alias) = match item.split_once(" as ") {
                Some((name, alias)) => (name.trim(), Some(alias.trim().to_string())),
                None => (item, None),
            };
            let glob = name == "*";
            ImportBinding {
                path: path.to_string(),
                name: (!glob).then(|| name.to_string()),
                alias: if glob { None } else { alias },
                glob,
                exported_as: None,
                site,
            }
        })
        .collect()
}

/// The alias of `import a.b`: the receiver spelling `a.b` in `a.b.f()` is the
/// whole dotted name, which is not the path's last segment. `None` for a name
/// with no dots, whose local name already is its last segment.
fn dotted(path: &str) -> Option<String> {
    path.contains('.').then(|| path.to_string())
}

/// The names after `import` in `from <path> import <names>`.
fn imported_names<'a>(text: &'a str, path: &str) -> Option<&'a str> {
    text.strip_prefix("from")?
        .trim_start()
        .strip_prefix(path)?
        .trim_start()
        .strip_prefix("import")
}

/// The statement on one line: comments, parentheses and line continuations
/// dropped, whitespace collapsed.
fn compact(statement: &str) -> String {
    statement
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .flat_map(|line| line.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '\\')))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
