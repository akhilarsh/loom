//! Human rendering of the source-item fields and the literal-text hint of a
//! pack, split out of `context.rs` to keep that file under the size limit.
//!
//! Every text here is untrusted-adjacent (an explanation quotes a seed id and a
//! path, a hint quotes the query), so each is neutralized: explanations go
//! through [`inline_safe`]; a window or a hint command is wrapped by the shared
//! renderer in a fence or code span that outgrows any backtick run in the
//! quoted text.

use crate::context::render::{render_literal_text_line, render_source_window};
use crate::context::schema::{ContextItem, ContextPack, ItemKind};
use crate::context::untrusted::inline_safe;

/// The lines a source item adds under its summary line: why a graph neighbour
/// was admitted, a trust caveat, and the attached source window. Empty for a
/// knowledge chunk and for a source item carrying none of the three.
pub(super) fn format_source_details(item: &ContextItem) -> String {
    if item.kind != ItemKind::SourceNode {
        return String::new();
    }
    let mut out = String::new();
    if let Some(explanation) = &item.explanation {
        out.push_str(&format!("          why: {}\n", inline_safe(explanation)));
    }
    if let Some(caveat) = &item.caveat {
        out.push_str(&format!("          caveat: {}\n", inline_safe(caveat)));
    }
    out.push_str(&render_source_window(item));
    out
}

/// The `Literal text:` line for a query that asked for text the graph does not
/// index, or `None` when the query asked for no such thing.
pub(super) fn format_text_search(pack: &ContextPack) -> Option<String> {
    pack.text_search.as_ref().map(render_literal_text_line)
}
