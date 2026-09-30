//! The bindings one TypeScript `import` or `export ... from` statement declares.
//!
//! A re-export binds no name in its own file, so its named forms carry
//! `alias: Some("")`; only `export * from` is a glob.

use crate::context::source_graph::{ImportBinding, Span};

/// The bindings of `statement`, whose module specifier is `path`, all at `site`.
pub(super) fn statement_bindings(statement: &str, path: &str, site: Span) -> Vec<ImportBinding> {
    let (is_export, clause) = split_clause(statement);
    let make = |name: Option<&str>, alias: Option<&str>, glob: bool| ImportBinding {
        path: path.to_string(),
        name: name.map(str::to_string),
        alias: alias.map(str::to_string),
        glob,
        site,
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
            // `import * as ns` binds the module as `ns`; `export * as ns` binds nothing.
            Some(Some(alias)) => bindings.push(make(None, local(is_export, alias.trim()), false)),
            // `import d from "x"`: the default export, bound as `d`.
            None => bindings.push(make(Some("default"), local(is_export, part), false)),
        }
    }
    for item in group
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let item = item.strip_prefix("type ").unwrap_or(item);
        let (name, alias) = match item.split_once(" as ") {
            Some((name, alias)) => (name.trim(), alias.trim()),
            None => (item, item),
        };
        bindings.push(make(
            Some(name.trim_matches(['"', '\''])),
            local(is_export, alias.trim_matches(['"', '\''])),
            false,
        ));
    }
    bindings
}

/// The local alias a name is bound under: itself for an import, nothing for a
/// re-export.
fn local(is_export: bool, name: &str) -> Option<&str> {
    Some(if is_export { "" } else { name })
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
