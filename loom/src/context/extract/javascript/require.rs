//! The bindings one CommonJS `require("path")` import declares.
//!
//! The query captures either a declarator (`util = require("./util")`,
//! `{ a, b: c } = require("./m")`) or a bare call statement (`require("./m")`).

use crate::context::source_graph::{ImportBinding, Span};

/// Whether `statement` is a `require` capture rather than an `import` or
/// `export ... from` statement. The keywords must be whole words: a declarator
/// named `importer` is still a `require`.
pub(super) fn is_require(statement: &str) -> bool {
    let trimmed = statement.trim_start();
    !["import", "export"].into_iter().any(|keyword| {
        trimmed
            .strip_prefix(keyword)
            .is_some_and(|rest| !rest.starts_with(is_identifier_char))
    })
}

fn is_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The bindings of a `require` capture whose module specifier is `path`, all at
/// `site`. A bare `require("m")` statement is a side-effect import
/// (`alias: Some("")`); `x = require("m")` binds the module as `x`; an object
/// pattern binds each named member.
pub(super) fn bindings(statement: &str, path: &str, site: Span) -> Vec<ImportBinding> {
    let make = |name: Option<&str>, alias: &str| ImportBinding {
        path: path.to_string(),
        name: name.map(str::to_string),
        alias: Some(alias.to_string()),
        glob: false,
        exported_as: None,
        site,
    };
    let Some(split) = find_top_level(statement, '=') else {
        return vec![make(None, "")];
    };
    let target = statement[..split].trim();
    if is_identifier(target) {
        return vec![make(None, target)];
    }
    let Some(members) = target
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        // An array pattern or anything else binds no name we can track.
        return vec![make(None, "")];
    };
    split_top_level(members, ',')
        .into_iter()
        .filter_map(|member| member_binding(member.trim()))
        .map(|(name, alias)| make(Some(name), alias))
        .collect()
}

/// `(exported name, local alias)` of one object-pattern member. A nested
/// pattern (`a: { b }`) binds no local name, so its alias is empty; a rest
/// element (`...others`) and a computed key bind nothing named.
fn member_binding(member: &str) -> Option<(&str, &str)> {
    if member.is_empty() || member.starts_with("...") || member.starts_with('[') {
        return None;
    }
    let (key, value) = match find_top_level(member, ':') {
        Some(colon) => (member[..colon].trim(), Some(member[colon + 1..].trim())),
        None => (member, None),
    };
    let key = without_default(key).trim_matches(['"', '\'']);
    if key.is_empty() {
        return None;
    }
    let local = value.map(without_default);
    Some(match local {
        None => (key, key),
        Some(local) if is_identifier(local) => (key, local),
        Some(_) => (key, ""),
    })
}

/// `text` without a trailing `= default` initializer.
fn without_default(text: &str) -> &str {
    match find_top_level(text, '=') {
        Some(equals) => text[..equals].trim(),
        None => text.trim(),
    }
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(is_identifier_char)
}

/// Byte index of the first `target` outside every bracket group and string
/// literal of `text`.
fn find_top_level(text: &str, target: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, c) in text.char_indices() {
        if let Some(open) = quote {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, _) if c == open => quote = None,
                _ => {}
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth = depth.saturating_sub(1),
            _ if c == target && depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

/// `text` split at every top-level `separator`.
fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(index) = find_top_level(rest, separator) {
        parts.push(&rest[..index]);
        rest = &rest[index + separator.len_utf8()..];
    }
    parts.push(rest);
    parts
}
