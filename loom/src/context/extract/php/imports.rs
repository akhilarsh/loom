//! PHP `use` declarations, read from the statement text.
//!
//! One declaration can bind several names (`use A\B, C\D;`, `use A\{B, C as D};`),
//! and a group's clause names only the tail of its path, so the shared walk's
//! one-path-per-match capture is completed from the statement here.

/// One name a `use` declaration binds.
pub(super) struct Clause {
    /// The full namespace path, written with `\` and without a leading `\`.
    pub(super) path: String,
    /// The `as` alias; `None` binds the path's last segment.
    pub(super) alias: Option<String>,
}

/// Whether `statement` is a `use` declaration, as opposed to a `require` or
/// `include` expression.
pub(super) fn is_use(statement: &str) -> bool {
    first_word(statement).is_some_and(|word| word.eq_ignore_ascii_case("use"))
}

/// Every clause of the `use` declaration `statement`, group members expanded
/// against the group's prefix.
pub(super) fn clauses(statement: &str) -> Vec<Clause> {
    let body = statement.trim().trim_end_matches(';');
    let body = strip_type(after_first_word(body));
    match body.split_once('{') {
        Some((prefix, group)) => {
            let prefix = prefix.trim().trim_matches('\\');
            group
                .trim_end_matches(['}', ' ', '\n', '\r', '\t'])
                .split(',')
                .filter_map(|item| clause(item, prefix))
                .collect()
        }
        None => body
            .split(',')
            .filter_map(|item| clause(item, ""))
            .collect(),
    }
}

/// The clause of `clauses(statement)` a captured `path` names: the one written
/// in full, else the one whose tail it is (a group member).
pub(super) fn find<'a>(clauses: &'a [Clause], path: &str) -> Option<&'a Clause> {
    let path = path.trim_start_matches('\\');
    clauses
        .iter()
        .find(|clause| clause.path == path)
        .or_else(|| {
            clauses
                .iter()
                .find(|clause| clause.path.ends_with(&format!("\\{path}")))
        })
}

/// `Name`, `Name as Alias` or `function Name` under `prefix`; `None` for an
/// empty item (a trailing comma).
fn clause(item: &str, prefix: &str) -> Option<Clause> {
    let item = strip_type(item.trim());
    let mut words = item.split_whitespace();
    let name = words.next()?.trim_start_matches('\\');
    let alias = match (words.next(), words.next()) {
        (Some(keyword), Some(alias)) if keyword.eq_ignore_ascii_case("as") => {
            Some(alias.to_string())
        }
        _ => None,
    };
    let path = if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}\\{name}")
    };
    Some(Clause { path, alias })
}

/// `text` without a leading `function` or `const` keyword.
fn strip_type(text: &str) -> &str {
    match first_word(text) {
        Some(word)
            if word.eq_ignore_ascii_case("function") || word.eq_ignore_ascii_case("const") =>
        {
            after_first_word(text)
        }
        _ => text,
    }
}

fn first_word(text: &str) -> Option<&str> {
    text.split_whitespace().next()
}

/// Everything after the first whitespace-separated word, left-trimmed.
fn after_first_word(text: &str) -> &str {
    text.trim_start()
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim_start())
}
