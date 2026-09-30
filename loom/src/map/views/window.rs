//! `--window`: the exact source lines of a node or call site.
//!
//! The command reads the window (its exit codes depend on the outcome); these
//! functions only present a window that was read while the file still matched
//! the snapshot, so the hash column is always `match`.

use serde_json::{json, Value};

use crate::context::window::SourceWindow;

use super::json::safe;
use super::snapshot::SnapshotIdentity;

/// `path:L<a>-L<b>  <state> <base rev8>[+<generation8>]  hash match`, then the
/// lines themselves.
pub fn render_window(window: &SourceWindow, snapshot: &SnapshotIdentity) -> String {
    let mut lines = vec![format!(
        "{}:L{}-L{}  {}  hash match",
        safe(&window.path),
        window.span.line_start,
        window.span.line_end,
        snapshot.window_label()
    )];
    lines.push(terminal_safe(&window.text));
    if window.truncated {
        lines.push("... truncated (raise --window-lines)".to_string());
    }
    lines.join("\n")
}

pub fn window_json(window: &SourceWindow, snapshot: &SnapshotIdentity) -> Value {
    json!({
        "path": safe(&window.path),
        "line_start": window.span.line_start,
        "line_end": window.span.line_end,
        "start_byte": window.span.start_byte,
        "end_byte": window.span.end_byte,
        "state": snapshot.state.as_str(),
        "base_revision": snapshot.base_revision,
        "generation": snapshot.generation,
        "hash": "match",
        "truncated": window.truncated,
        "text": window.text,
    })
}

/// Source text is repo-controlled: keep every character a reader needs (tabs
/// and newlines included) but replace the control characters that would drive
/// the terminal, such as the ESC that starts an ANSI sequence.
fn terminal_safe(text: &str) -> String {
    text.replace("\r\n", "\n")
        .trim_end_matches('\n')
        .chars()
        .map(|ch| {
            if ch.is_control() && ch != '\n' && ch != '\t' {
                '\u{FFFD}'
            } else {
                ch
            }
        })
        .collect()
}
