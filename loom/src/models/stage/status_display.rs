//! Display helpers of [`StageStatus`]: icon, colours, label and status bucket.

use super::types::{StageStatus, StatusBucket};

impl StageStatus {
    /// Returns the icon character for this status
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Completed => "\u{2713}",      // ✓
            Self::Executing => "\u{25CF}",      // ●
            Self::Queued => "\u{25B6}",         // ▶
            Self::WaitingForDeps => "\u{25CB}", // ○
            Self::WaitingForInput => "?",
            Self::Blocked => "\u{2717}",               // ✗
            Self::NeedsHandoff => "\u{27F3}",          // ⟳
            Self::Skipped => "\u{2298}",               // ⊘
            Self::MergeConflict => "\u{26A1}",         // ⚡
            Self::CompletedWithFailures => "\u{26A0}", // ⚠
            Self::MergeBlocked => "\u{2297}",          // ⊗
            Self::NeedsHumanReview => "\u{23F8}",      // ⏸
            Self::NeedsAdjudication => "\u{2696}",     // ⚖
        }
    }

    /// Returns the terminal color for this status (for the `colored` crate)
    pub fn terminal_color(&self) -> colored::Color {
        use colored::Color;
        match self {
            Self::Completed => Color::Green,
            Self::Executing => Color::Blue,
            Self::Queued => Color::Cyan,
            Self::WaitingForDeps => Color::White,
            Self::WaitingForInput => Color::Magenta,
            Self::Blocked => Color::Red,
            Self::NeedsHandoff => Color::Yellow,
            Self::Skipped => Color::White,
            Self::MergeConflict => Color::Yellow,
            Self::CompletedWithFailures => Color::Red,
            Self::MergeBlocked => Color::Red,
            Self::NeedsHumanReview => Color::Magenta,
            Self::NeedsAdjudication => Color::Yellow,
        }
    }

    /// Returns whether this status should be bold
    pub fn is_bold(&self) -> bool {
        // Bold by default except for low-attention states.
        // NeedsAdjudication is bold (active attention needed).
        !matches!(
            self,
            Self::WaitingForDeps | Self::Skipped | Self::NeedsHumanReview
        )
    }

    /// Returns whether this status should be dimmed
    pub fn is_dimmed(&self) -> bool {
        matches!(self, Self::WaitingForDeps)
    }

    /// Returns whether this status should be strikethrough
    pub fn is_strikethrough(&self) -> bool {
        matches!(self, Self::Skipped)
    }

    /// Returns the ratatui style for this status
    pub fn tui_style(&self) -> ratatui::style::Style {
        use ratatui::style::{Color, Modifier, Style};
        let mut style = Style::default();

        let color = match self {
            Self::Completed => Color::Green,
            Self::Executing => Color::Blue,
            Self::Queued => Color::Cyan,
            Self::WaitingForDeps => Color::Gray,
            Self::WaitingForInput => Color::Magenta,
            Self::Blocked => Color::Red,
            Self::NeedsHandoff => Color::Yellow,
            Self::Skipped => Color::DarkGray,
            Self::MergeConflict => Color::Yellow,
            Self::CompletedWithFailures => Color::Red,
            Self::MergeBlocked => Color::Red,
            Self::NeedsHumanReview => Color::Magenta,
            Self::NeedsAdjudication => Color::Yellow,
        };
        style = style.fg(color);

        if self.is_bold() {
            style = style.add_modifier(Modifier::BOLD);
        }

        style
    }

    /// Returns the authoritative short label for this status.
    ///
    /// This is the single source of truth for the compact status label used by
    /// every renderer (status summary, completion table, TUI). Renderers MUST
    /// call this rather than hand-rolling their own match — past divergence
    /// (`"MergeErr"` here vs `"MergeBlk"` in two renderers) is exactly the bug
    /// this consolidation prevents.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Completed => "Completed",
            Self::Executing => "Executing",
            Self::Queued => "Queued",
            Self::WaitingForDeps => "Waiting",
            Self::WaitingForInput => "Input",
            Self::Blocked => "Blocked",
            Self::NeedsHandoff => "Handoff",
            Self::Skipped => "Skipped",
            Self::MergeConflict => "Conflict",
            Self::CompletedWithFailures => "Failed",
            Self::MergeBlocked => "MergeBlk",
            Self::NeedsHumanReview => "Review",
            Self::NeedsAdjudication => "Adjudicate",
        }
    }

    /// Classify this status into a coarse [`StatusBucket`].
    ///
    /// The mapping matches the established daemon/CLI semantics:
    /// - `NeedsHandoff` and `WaitingForInput` are **Executing** — they are active
    ///   states where work is ongoing (per the existing daemon comment at
    ///   `daemon/server/status.rs`: "NeedsHandoff and WaitingForInput are active
    ///   states where work is ongoing, so they belong in executing").
    /// - `Skipped` is grouped with `Completed` (terminal, not blocked).
    /// - All merge-failure and review/adjudication states are **Blocked** (stopped,
    ///   needing attention).
    pub fn bucket(&self) -> StatusBucket {
        match self {
            Self::Executing | Self::NeedsHandoff | Self::WaitingForInput => StatusBucket::Executing,
            Self::WaitingForDeps | Self::Queued => StatusBucket::Pending,
            Self::Completed | Self::Skipped => StatusBucket::Completed,
            Self::Blocked
            | Self::MergeConflict
            | Self::CompletedWithFailures
            | Self::MergeBlocked
            | Self::NeedsHumanReview
            | Self::NeedsAdjudication => StatusBucket::Blocked,
        }
    }
}
