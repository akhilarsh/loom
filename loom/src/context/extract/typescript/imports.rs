//! The bindings one TypeScript `import` or `export ... from` statement declares.
//!
//! A re-export binds no name in its own file, so its named forms carry
//! `alias: Some("")` and record the name the module exports in `exported_as`;
//! only `export * from` is a glob.

use crate::context::source_graph::{ImportBinding, Span};

/// The bindings of `statement`, whose module specifier is `path`, all at `site`.
pub(super) fn statement_bindings(statement: &str, path: &str, site: Span) -> Vec<ImportBinding> {
    let (is_export, clause) = split_clause(statement);
    let make = |name: Option<&str>, alias: Option<&str>, glob: bool| ImportBinding {
        path: path.to_string(),
        name: name.map(str::to_string),
        alias: alias.map(str::to_string),
        glob,
        exported_as: None,
        site,
    };
    // `name` from the module, known here as `bound`: the local name of an
    // import, the exported name of a re-export.
    let named = |name: Option<&str>, bound: &str| ImportBinding {
        exported_as: is_export.then(|| bound.to_string()),
        ..make(name, Some(if is_export { "" } else { bound }), false)
    };
    if clause.is_empty() {
        // `import "x"`: evaluated for its effects, binds nothing.
        return vec![make(None, Some(""), false)];
    }

    let (before, group, after) = split_group(&clause);
    let mut bindings = Vec::new();
    for part in before.split(',').chain(after.split(',')) {
        let part = part.trim();
        match part
            .strip_prefix('*')
            .map(|rest| rest.trim().strip_prefix("as "))
        {
            None if part.is_empty() => {}
            // `export * from "x"`.
            Some(None) => bindings.push(make(None, None, true)),
            // `import * as ns` binds the module as `ns`; `export * as ns` exports it.
            Some(Some(alias)) => bindings.push(named(None, alias.trim())),
            // `import d from "x"`: the default export, bound as `d`.
            None => bindings.push(named(Some("default"), part)),
        }
    }
    for (name, bound) in group_items(group) {
        bindings.push(named(Some(name), bound));
    }
    bindings
}

/// Each item of a `{ ... }` group as `(name, bound)`: `a` gives `(a, a)` and
/// `a as b` gives `(a, b)`, `type` markers and quotes dropped.
fn group_items(group: &str) -> impl Iterator<Item = (&str, &str)> {
    group
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            let item = item.strip_prefix("type ").unwrap_or(item);
            let (name, bound) = item
                .split_once(" as ")
                .map_or((item, item), |(name, bound)| (name.trim(), bound.trim()));
            (
                name.trim_matches(['"', '\'']),
                bound.trim_matches(['"', '\'']),
            )
        })
}

/// Whether the statement is an `export`, and the text between its keyword and
/// the module specifier, whitespace collapsed and `type` markers dropped.
fn split_clause(statement: &str) -> (bool, String) {
    let trimmed = statement.trim_start();
    let (is_export, rest) = match trimmed.strip_prefix("export") {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix("import").unwrap_or(trimmed)),
    };
    // The specifier is the first quote outside a brace group.
    let mut depth = 0usize;
    let end = rest
        .char_indices()
        .find_map(|(index, character)| match character {
            '{' => {
                depth += 1;
                None
            }
            '}' => {
                depth = depth.saturating_sub(1);
                None
            }
            '"' | '\'' if depth == 0 => Some(index),
            _ => None,
        })
        .unwrap_or(rest.len());
    let before = rest[..end].trim_end();
    let before = before.strip_suffix("from").unwrap_or(before);
    let clause = before.split_whitespace().collect::<Vec<_>>().join(" ");
    let clause = clause.strip_prefix("type ").unwrap_or(&clause).to_string();
    (is_export, clause)
}

/// A clause split around its one `{ ... }` group: the text before it, the
/// group's inside, and the text after it. Without a group the whole clause is
/// the first part.
fn split_group(clause: &str) -> (&str, &str, &str) {
    match (clause.find('{'), clause.rfind('}')) {
        (Some(open), Some(close)) if open < close => (
            &clause[..open],
            &clause[open + 1..close],
            &clause[close + 1..],
        ),
        _ => (clause, "", ""),
    }
}
