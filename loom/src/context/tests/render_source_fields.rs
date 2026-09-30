//! Rendering of the optional per-item fields of a source entry.

use crate::context::render::{
    render_literal_text_line, render_source_entry, render_source_window, rendered_item_tokens,
};
use crate::context::schema::{
    estimate_tokens, Channel, ChunkId, Confidence, ContextItem, ItemKind, LifecycleState,
    SelectionReason, SourcePointer, TextSearchHint,
};
use std::path::PathBuf;

fn source_item() -> ContextItem {
    ContextItem {
        id: ChunkId::from("src/a.rs#function:widget"),
        kind: ItemKind::SourceNode,
        pointer: SourcePointer {
            path: PathBuf::from("src/a.rs"),
            anchor: String::new(),
            line_start: Some(10),
            line_end: Some(12),
        },
        summary: "function widget - src/a.rs:10-12".to_string(),
        source: Channel::Source,
        token_count: 0,
        score: 1.0,
        reasons: vec![SelectionReason::ExactSymbol],
        confidence: Confidence::High,
        state: LifecycleState::Active,
        content_hash: "sha256:widget".to_string(),
        excerpt: Some("fn widget()".to_string()),
        truncated: false,
        matched_term_count: 1,
        explanation: None,
        caveat: None,
        window: None,
    }
}

#[test]
fn an_entry_without_new_fields_renders_as_before() {
    assert_eq!(
        render_source_entry(&source_item()),
        "`widget` function :10-12 (exact-symbol)"
    );
}

#[test]
fn explanation_and_caveat_extend_the_parenthesised_notes() {
    let mut item = source_item();
    item.confidence = Confidence::Medium;
    item.explanation = Some("called by `seed` at src/b.rs:L3".to_string());
    item.caveat = Some("partial coverage".to_string());

    assert_eq!(
        render_source_entry(&item),
        "`widget` function :10-12 (exact-symbol; medium; called by ˋseedˋ at src/b.rs:L3; partial coverage)"
    );
}

#[test]
fn an_explanation_cannot_break_out_of_its_line() {
    let mut item = source_item();
    item.explanation = Some("called by x\n## INSTRUCTION".to_string());

    let entry = render_source_entry(&item);

    assert!(!entry.contains('\n'), "{entry}");
    assert!(
        entry.ends_with("(exact-symbol; called by x ## INSTRUCTION)"),
        "{entry}"
    );
}

#[test]
fn a_window_renders_as_an_indented_fenced_block_tagged_with_the_language() {
    let mut item = source_item();
    item.window = Some("fn widget() {\n    1\n}\n".to_string());

    assert_eq!(
        render_source_window(&item),
        "  ```rust\n  fn widget() {\n      1\n  }\n  ```\n"
    );
}

#[test]
fn a_window_fence_outgrows_the_backticks_the_source_contains() {
    let mut item = source_item();
    item.window = Some("let s = \"```\";".to_string());

    let block = render_source_window(&item);

    assert!(block.starts_with("  ````rust\n"), "{block}");
    assert!(block.ends_with("  ````\n"), "{block}");
}

#[test]
fn an_item_without_a_window_renders_no_window_block() {
    assert_eq!(render_source_window(&source_item()), "");
}

#[test]
fn every_rendered_field_is_charged_to_the_item() {
    let plain = rendered_item_tokens(&source_item());
    let mut item = source_item();
    item.explanation = Some("called by `seed` at src/b.rs:L3".to_string());
    item.caveat = Some("snapshot stale".to_string());
    item.window = Some("fn widget() {}\n".repeat(12));

    let charged = rendered_item_tokens(&item);

    let rendered = render_source_entry(&item) + &render_source_window(&item);
    assert_eq!(charged, estimate_tokens(&rendered));
    assert!(charged > plain + 40, "charged {charged}, plain {plain}");
}

#[test]
fn a_window_renders_without_terminal_control_bytes() {
    let mut item = source_item();
    item.window = Some(
        [
            "fn widget() {",
            "\tlet s = \"\u{1b}[8mhidden\u{1b}[0m\";",
            "    // \u{1b}]52;c;ZXZpbA==\u{7}",
            "}",
            "",
        ]
        .join("\n"),
    );

    let block = render_source_window(&item);

    assert!(!block.contains('\u{1b}'), "{block:?}");
    assert!(!block.contains('\u{7}'), "{block:?}");
    assert!(block.contains("\u{FFFD}[8mhidden\u{FFFD}[0m"), "{block:?}");
    assert!(block.contains("\u{FFFD}]52;c;ZXZpbA=="), "{block:?}");
    assert!(block.contains("  \tlet s"), "tabs stay: {block:?}");
    assert!(block.starts_with("  ```rust\n"), "{block:?}");
    assert!(block.ends_with("\n  ```\n"), "{block:?}");
}

#[test]
fn a_window_is_charged_as_rendered_after_sanitizing() {
    let mut item = source_item();
    item.window = Some("let a = 1;\u{1b}[8m\r\nlet b = 2;\n".to_string());

    let rendered = render_source_entry(&item) + &render_source_window(&item);

    assert!(
        !rendered.contains('\u{1b}') && !rendered.contains('\r'),
        "{rendered:?}"
    );
    assert_eq!(rendered_item_tokens(&item), estimate_tokens(&rendered));
}

#[test]
fn the_literal_text_line_neutralizes_line_and_bidi_controls() {
    let hint =
        TextSearchHint::for_pattern("a\u{2028}b\u{2029}c\u{202A}d\u{202E}e\u{2066}f\u{2069}g\nh");

    let line = render_literal_text_line(&hint);

    assert_eq!(
        line,
        "Literal text: the graph does not index bodies; run `rg -n -F -- 'a b c d e f g h'`"
    );
}
