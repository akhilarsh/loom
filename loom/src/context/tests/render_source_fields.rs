//! Rendering of the optional per-item fields of a source entry.

use crate::context::render::{render_source_entry, render_source_window, rendered_item_tokens};
use crate::context::schema::{
    estimate_tokens, Channel, ChunkId, Confidence, ContextItem, ItemKind, LifecycleState,
    SelectionReason, SourcePointer,
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
