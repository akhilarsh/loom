//! The bindings one Rust `use` argument declares.
//!
//! The argument is a use tree, `a::b::c`, `a::b as d`, `a::*`, or a brace group
//! of those, nested up to `MAX_USE_TREE_DEPTH` groups deep. It is parsed from
//! its text, one binding per leaf with the group prefixes applied.

use crate::context::source_graph::{ImportBinding, Span};

/// Brace groups `expand` descends through. Source files are untrusted input, so
/// recursion and the per-level scans of the remaining text must stay bounded;
/// real use trees nest two or three levels. A group past the cap contributes no
/// bindings: a dropped import leaves its calls unresolved rather than misbound.
pub(super) const MAX_USE_TREE_DEPTH: usize = 16;

/// One binding per leaf of the use tree `argument`, all at `site`.
pub(super) fn use_bindings(argument: &str, site: Span) -> Vec<ImportBinding> {
    let mut bindings = Vec::new();
    expand(&compact(argument), "", 0, site, &mut bindings);
    bindings
}

/// Expand `tree` under `prefix`: a brace group recurses per item, anything
/// else is a leaf. `depth` counts the groups already entered; a group beyond
/// `MAX_USE_TREE_DEPTH` binds nothing.
fn expand(tree: &str, prefix: &str, depth: usize, site: Span, bindings: &mut Vec<ImportBinding>) {
    let Some((head, group)) = split_group(tree) else {
        bindings.push(leaf(tree, prefix, site));
        return;
    };
    if depth >= MAX_USE_TREE_DEPTH {
        return;
    }
    let prefix = join(prefix, head.trim_end_matches("::"));
    for item in top_level_items(group) {
        expand(item, &prefix, depth + 1, site, bindings);
    }
}

/// The text before the first `{` and the text between it and the last `}`.
fn split_group(tree: &str) -> Option<(&str, &str)> {
    let open = tree.find('{')?;
    let close = tree.rfind('}')?;
    (open < close).then(|| (&tree[..open], &tree[open + 1..close]))
}

/// The comma-separated items of a group, ignoring commas inside nested groups.
fn top_level_items(group: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, character) in group.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(&group[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    items.push(&group[start..]);
    items
        .into_iter()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

/// The binding of one leaf: `c`, `c as d`, `*`, `self`, or `self as m`.
fn leaf(tree: &str, prefix: &str, site: Span) -> ImportBinding {
    let (target, alias) = match tree.split_once(" as ") {
        // `as _` imports a trait for its methods and binds no name.
        Some((target, "_")) => (target, Some(String::new())),
        Some((target, alias)) => (target, Some(alias.to_string())),
        None => (tree, None),
    };
    if target.ends_with('*') {
        return ImportBinding {
            path: join(prefix, target.trim_end_matches('*').trim_end_matches("::")),
            name: None,
            alias: None,
            glob: true,
            exported_as: None,
            site,
        };
    }
    // `self` inside a group binds the group's module itself.
    let (path, name) = if target == "self" {
        (prefix.to_string(), None)
    } else {
        let last = target.rsplit("::").next().unwrap_or(target);
        (join(prefix, target), Some(last.to_string()))
    };
    ImportBinding {
        path,
        name,
        alias,
        glob: false,
        exported_as: None,
        site,
    }
}

/// `prefix::rest`, or whichever side is non-empty.
fn join(prefix: &str, rest: &str) -> String {
    match (prefix.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (_, true) => prefix.to_string(),
        _ => format!("{prefix}::{rest}"),
    }
}

/// Collapse whitespace runs and drop the spaces around delimiters, so a use
/// tree wrapped over several lines reads like its one-line spelling.
fn compact(text: &str) -> String {
    let mut out = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for delimiter in ["::", "{", "}", ","] {
        out = out
            .replace(&format!(" {delimiter}"), delimiter)
            .replace(&format!("{delimiter} "), delimiter);
    }
    out
}
