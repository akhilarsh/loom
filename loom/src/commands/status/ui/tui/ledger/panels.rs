use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::legend::LEGEND;
use super::text::{self, cut_line, spans_width};
use super::{quota, LedgerView};
use crate::commands::status::render::attention_model::{
    failure_label, human_review_choices, AttentionEntry,
};
use crate::commands::status::ui::theme::{StatusColors, Theme};
use crate::commands::status::ui::tui::state::TuiActivityLog;
use crate::models::stage::StageStatus;

/// Indent of the lines under an entry's header line.
const DETAIL_INDENT: &str = "                       ";

/// Render the needs-attention panel.
pub fn render_attention(frame: &mut Frame, area: Rect, entries: &[AttentionEntry]) {
    frame.render_widget(Paragraph::new(attention_lines(entries, area.width)), area);
}

/// Render the activity panel.
pub fn render_activity(frame: &mut Frame, area: Rect, log: &TuiActivityLog) {
    let mut lines = vec![cut_line(
        Line::from(Span::styled(
            " ACTIVITY",
            Theme::status_pending().add_modifier(Modifier::BOLD),
        )),
        area.width,
    )];
    if log.is_empty() {
        lines.push(cut_line(
            Line::from(Span::styled("Waiting for events...", Theme::dimmed())),
            area.width,
        ));
    } else {
        lines.extend(
            log.render_lines(area.height.saturating_sub(1) as usize)
                .into_iter()
                .map(|line| cut_line(line, area.width)),
        );
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// Render the footer: the quota line when the area has a row for it, above the legend
/// strip with its key hints, or the last error in the strip's place.
pub fn render_footer(frame: &mut Frame, area: Rect, present: &[StageStatus], view: &LedgerView) {
    let mut lines = Vec::new();
    if area.height >= 2 {
        lines.push(quota::quota_line(
            &view.data.quota,
            view.now_epoch,
            area.width,
        ));
    }
    lines.push(view.last_error.map_or_else(
        || footer_line(present, area.width, view.scrollable),
        |message| {
            cut_line(
                Line::from(Span::styled(
                    format!("Error: {message}"),
                    Theme::status_blocked(),
                )),
                area.width,
            )
        },
    ));
    frame.render_widget(Paragraph::new(lines), area);
}

/// Build the needs-attention title and detailed entry lines.
pub fn attention_lines(entries: &[AttentionEntry], width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![cut_line(
        Line::from(Span::styled(
            " NEEDS ATTENTION",
            Theme::status_pending().add_modifier(Modifier::BOLD),
        )),
        width,
    )];
    for entry in entries {
        lines.extend(entry_lines(entry, width));
    }
    lines
}

/// Lines the entries take in the needs-attention panel, below its title.
pub fn attention_line_count(entries: &[AttentionEntry], width: u16) -> usize {
    entries
        .iter()
        .map(|entry| entry_lines(entry, width).len())
        .sum()
}

/// Build the legend strip and right-aligned key hints for the footer; the scroll hint
/// is only offered when the table overflows its viewport.
pub fn footer_line(present: &[StageStatus], width: u16, scrollable: bool) -> Line<'static> {
    let keys = key_hints(scrollable);
    let key_width = spans_width(&keys);
    let mut entries: Vec<_> = LEGEND
        .iter()
        .filter(|(status, _)| present.contains(status))
        .map(|(status, _)| legend_entry(status))
        .collect();
    while entries_width(&entries) + key_width > width as usize {
        if entries.pop().is_none() {
            break;
        }
    }
    let mut spans = join_entries(entries);
    let gap = (width as usize).saturating_sub(spans_width(&spans) + key_width);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(keys);
    cut_line(Line::from(spans), width)
}

fn entry_lines(entry: &AttentionEntry, width: u16) -> Vec<Line<'static>> {
    let status = entry_status(entry.label);
    let detail = attention_detail(entry);
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(status.icon().to_owned(), status.tui_style()),
        Span::raw(format!(" {} ", text::padded(&entry.id, 20))),
        Span::styled(
            entry.label.to_owned(),
            status.tui_style().add_modifier(Modifier::BOLD),
        ),
    ];
    if !detail.is_empty() {
        spans.push(Span::raw(" · "));
        spans.push(Span::raw(detail));
    }
    let mut lines = vec![cut_line(Line::from(spans), width)];
    if let Some(evidence) = entry.evidence.first() {
        lines.push(cut_line(
            Line::from(Span::styled(
                format!("{DETAIL_INDENT}{evidence}"),
                Theme::dimmed(),
            )),
            width,
        ));
    }
    lines.extend(guidance_line(entry, width));
    if entry.has_human_review_choices {
        lines.extend(
            human_review_choices(&entry.id)
                .into_iter()
                .map(|(command, _)| command_line(command, Vec::new(), width)),
        );
    }
    lines
}

/// `cleanup_warning` is already flattened to one line (and capped at
/// `MAX_INLINE_CHARS`) by `context::untrusted::inline_safe` before it reaches
/// a `StageSummary`, so it is used directly rather than split on newlines.
fn attention_detail(entry: &AttentionEntry) -> String {
    entry
        .review_reason
        .clone()
        .or_else(|| {
            entry
                .failure_type
                .as_ref()
                .map(failure_label)
                .map(str::to_owned)
        })
        .or_else(|| entry.cleanup_warning.clone())
        .unwrap_or_default()
}

/// The entry's command, followed by its note, on one line; a note alone is
/// prose, so it gets no command arrow. `None` when the entry has neither.
fn guidance_line(entry: &AttentionEntry, width: u16) -> Option<Line<'static>> {
    let note = entry
        .note
        .clone()
        .map(|note| Span::styled(note, Theme::dimmed()));
    let Some(command) = entry.command.clone() else {
        return note.map(|note| cut_line(Line::from(vec![Span::raw(DETAIL_INDENT), note]), width));
    };
    let trailing = note.map_or_else(Vec::new, |note| {
        vec![Span::styled(" · ", Theme::dimmed()), note]
    });
    Some(command_line(command, trailing, width))
}

/// `→ command` under the entry's detail, followed by `trailing` spans.
fn command_line(command: String, trailing: Vec<Span<'static>>, width: u16) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{DETAIL_INDENT}→ "), Theme::dimmed()),
        Span::styled(command, Style::default().fg(StatusColors::QUEUED)),
    ];
    spans.extend(trailing);
    cut_line(Line::from(spans), width)
}

fn entry_status(label: &str) -> StageStatus {
    match label {
        "MERGE CONFLICT" => StageStatus::MergeConflict,
        "ACCEPTANCE FAILED" => StageStatus::CompletedWithFailures,
        "MERGE ERROR" | "MERGE BLOCKED" => StageStatus::MergeBlocked,
        // Changes at risk read as the strongest state.
        "STASH NOT RESTORED" => StageStatus::Blocked,
        "NEEDS REVIEW" => StageStatus::NeedsHumanReview,
        "NEEDS INPUT" => StageStatus::WaitingForInput,
        "ADJUDICATING" => StageStatus::NeedsAdjudication,
        _ => StageStatus::Blocked,
    }
}

fn legend_entry(status: &StageStatus) -> Vec<Span<'static>> {
    vec![
        Span::styled(status.icon().to_owned(), status.tui_style()),
        Span::styled(format!(" {}", footer_label(status)), Theme::dimmed()),
    ]
}

fn footer_label(status: &StageStatus) -> String {
    if *status == StageStatus::Completed {
        "done".to_owned()
    } else {
        status.label().to_ascii_lowercase()
    }
}

fn key_hints(scrollable: bool) -> Vec<Span<'static>> {
    let key = Style::default().add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::styled("?", key),
        Span::styled(" legend", Theme::dimmed()),
    ];
    if scrollable {
        spans.push(Span::styled(" · ", Theme::dimmed()));
        spans.push(Span::styled("↑↓", key));
        spans.push(Span::styled(" scroll", Theme::dimmed()));
    }
    spans.push(Span::styled(" · ", Theme::dimmed()));
    spans.push(Span::styled("q", key));
    spans.push(Span::styled(" quit", Theme::dimmed()));
    spans
}

fn join_entries(entries: Vec<Vec<Span<'static>>>) -> Vec<Span<'static>> {
    entries
        .into_iter()
        .enumerate()
        .flat_map(|(index, entry)| {
            if index == 0 {
                entry
            } else {
                let mut separated = vec![Span::raw("  ")];
                separated.extend(entry);
                separated
            }
        })
        .collect()
}

fn entries_width(entries: &[Vec<Span<'static>>]) -> usize {
    entries
        .iter()
        .map(|entry| spans_width(entry))
        .sum::<usize>()
        + entries.len().saturating_sub(1) * 2
}

#[cfg(test)]
mod tests {
    use super::{attention_line_count, attention_lines, footer_line, AttentionEntry};
    use crate::models::stage::StageStatus;

    fn entry(label: &'static str, command: Option<&str>, note: Option<&str>) -> AttentionEntry {
        AttentionEntry {
            id: "stage-a".into(),
            name: "Stage A".into(),
            label,
            command: command.map(str::to_owned),
            note: note.map(str::to_owned),
            automatic: false,
            failure_type: None,
            evidence: Vec::new(),
            review_reason: None,
            cleanup_warning: None,
            has_human_review_choices: false,
            dispute_count: None,
            judge_heartbeat_secs: None,
            completion_blocker: None,
            outgoing_session_exit_reason: None,
        }
    }

    fn rendered(entry: AttentionEntry) -> Vec<String> {
        attention_lines(&[entry], 120)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn human_review_attention_lists_the_three_full_commands() {
        let lines = rendered(AttentionEntry {
            review_reason: Some("ambiguous result".into()),
            has_human_review_choices: true,
            ..entry("NEEDS REVIEW", None, None)
        });
        assert_eq!(lines.len(), 5, "{lines:#?}");
        assert!(lines[2].ends_with("→ loom stage human-review stage-a --approve"));
        assert!(lines[3].ends_with("→ loom stage human-review stage-a --force-complete"));
        assert!(lines[4].ends_with("→ loom stage human-review stage-a --reject \"<reason>\""));
    }

    #[test]
    fn entry_without_detail_omits_the_dangling_separator() {
        let lines = rendered(entry("NEEDS INPUT", None, Some("answer it")));
        assert!(lines[1].ends_with("NEEDS INPUT"));
        assert!(!lines[1].contains(" · "));
    }

    #[test]
    fn a_note_alone_has_no_command_arrow_and_follows_a_command() {
        let note_only = rendered(entry("NEEDS INPUT", None, Some("answer it")));
        assert_eq!(note_only[2].trim(), "answer it");
        let both = rendered(entry(
            "BLOCKED",
            Some("loom stage retry stage-a --force"),
            Some("retry limit reached (3/3)"),
        ));
        let expected = "→ loom stage retry stage-a --force · retry limit reached (3/3)";
        assert!(both[2].ends_with(expected), "{both:#?}");
    }

    #[test]
    fn line_count_is_the_rendered_entry_lines() {
        let review = AttentionEntry {
            has_human_review_choices: true,
            ..entry("NEEDS REVIEW", None, None)
        };
        let blocked = entry("BLOCKED", Some("loom stage retry stage-a"), None);

        assert_eq!(attention_line_count(&[review, blocked], 120), 6);
    }

    #[test]
    fn an_entry_with_neither_command_nor_note_prints_no_empty_line() {
        let lines = rendered(entry("NEEDS REVIEW", None, None));
        assert_eq!(lines.len(), 2, "{lines:#?}");
    }

    #[test]
    fn footer_only_lists_present_statuses_and_keeps_quit_hint() {
        let present = [StageStatus::Executing, StageStatus::Completed];
        let line = footer_line(&present, 100, true).to_string();
        assert!(line.contains("executing"));
        assert!(line.contains("done"));
        assert!(!line.contains("queued"));
        assert!(footer_line(&present, 40, true)
            .to_string()
            .ends_with("q quit"));
    }

    #[test]
    fn scroll_hint_appears_only_when_the_table_overflows() {
        let present = [StageStatus::Executing];
        let scrollable = footer_line(&present, 100, true).to_string();
        let fixed = footer_line(&present, 100, false).to_string();
        assert!(scrollable.contains("↑↓ scroll"));
        assert!(!fixed.contains("↑↓"));
        assert!(fixed.ends_with("? legend · q quit"));
    }
}
