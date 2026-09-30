//! State types for the TUI application.

use std::collections::{HashMap, HashSet, VecDeque};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::commands::status::data::{
    CompletionBlockerState, CompletionBlockerSummary, StageSummary, StatusData,
};
use crate::commands::status::ui::theme::Theme;
use crate::models::stage::StageStatus;
use crate::plan::graph::levels;

/// Graph state tracking for scroll position.
#[derive(Default)]
pub struct GraphState {
    /// Vertical scroll offset for the tree.
    pub scroll_y: u16,
    /// Total number of lines in the tree.
    pub total_lines: u16,
    /// Viewport height for scrolling bounds.
    pub viewport_height: u16,
}

impl GraphState {
    /// Scroll by a delta, clamping to bounds.
    pub fn scroll_by(&mut self, delta: i16) {
        if delta < 0 {
            self.scroll_y = self.scroll_y.saturating_sub((-delta) as u16);
        } else {
            let max_scroll = self.total_lines.saturating_sub(self.viewport_height);
            self.scroll_y = self.scroll_y.saturating_add(delta as u16).min(max_scroll);
        }
    }

    /// Jump to start.
    pub fn scroll_to_start(&mut self) {
        self.scroll_y = 0;
    }

    /// Jump to end.
    pub fn scroll_to_end(&mut self) {
        self.scroll_y = self.total_lines.saturating_sub(self.viewport_height);
    }
}

/// Live status data received from daemon.
#[derive(Default)]
pub struct LiveStatus {
    pub data: StatusData,
}

impl LiveStatus {
    /// Compute execution levels for all stages based on dependencies.
    pub fn compute_levels(&self) -> HashMap<String, usize> {
        levels::compute_all_levels(&self.data.stages, |s| s.id.as_str(), |s| &s.dependencies)
    }

    /// Collect all stages into a deduplicated list, sorted by level then id.
    pub fn all_stages(&self) -> Vec<&StageSummary> {
        self.all_stages_with_levels(&self.compute_levels())
    }

    /// Same as `all_stages`, but reuses an already-computed level map instead
    /// of recomputing it - the caller already needs both.
    pub fn all_stages_with_levels(&self, levels: &HashMap<String, usize>) -> Vec<&StageSummary> {
        let mut stages = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for stage in &self.data.stages {
            if seen.insert(stage.id.clone()) {
                stages.push(stage);
            }
        }

        stages.sort_by(|a, b| {
            let la = levels.get(&a.id).copied().unwrap_or(0);
            let lb = levels.get(&b.id).copied().unwrap_or(0);
            la.cmp(&lb).then_with(|| a.id.cmp(&b.id))
        });

        stages
    }
}

/// A single activity log entry for the TUI.
pub struct TuiActivityEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub icon: &'static str,
    pub message: String,
    pub style: Style,
}

/// Activity log that tracks stage state transitions for the TUI.
pub struct TuiActivityLog {
    entries: VecDeque<TuiActivityEntry>,
    previous: HashMap<String, StageActivityState>,
}

struct StageActivityState {
    status: StageStatus,
    blocker_state: Option<CompletionBlockerState>,
    blocker_fingerprint: Option<String>,
}

impl TuiActivityLog {
    const MAX_ENTRIES: usize = 20;

    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            previous: HashMap::new(),
        }
    }

    /// Update the log by comparing current stage statuses against previous.
    /// Only logs meaningful transitions (started, completed, blocked, ready).
    pub fn update(&mut self, stages: &[&StageSummary]) {
        let now = chrono::Utc::now();

        for stage in stages {
            let (status_changed, completion_changed) = {
                let previous = self.previous.get(&stage.id);
                (
                    previous
                        .map(|state| state.status != stage.status)
                        .unwrap_or(true),
                    blocker_changed(previous, stage.completion_blocker.as_ref()),
                )
            };
            if status_changed {
                if let Some(entry) = status_entry(stage, now) {
                    self.push(entry);
                }
            }
            if completion_changed {
                if let Some(entry) = blocker_entry(stage, now) {
                    self.push(entry);
                }
            }
            self.previous
                .insert(stage.id.clone(), StageActivityState::from(*stage));
        }
    }

    fn push(&mut self, entry: TuiActivityEntry) {
        self.entries.push_back(entry);
        while self.entries.len() > Self::MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    /// Render the most recent entries as TUI Lines, oldest first.
    pub fn render_lines(&self, count: usize) -> Vec<Line<'static>> {
        self.entries
            .iter()
            .rev()
            .take(count)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|entry| {
                let time_str = entry.timestamp.format("%H:%M:%S").to_string();
                Line::from(vec![
                    Span::styled(time_str, Theme::dimmed()),
                    Span::raw("  "),
                    Span::styled(entry.icon.to_string(), entry.style),
                    Span::raw(" "),
                    Span::styled(entry.message.clone(), entry.style),
                ])
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl From<&StageSummary> for StageActivityState {
    fn from(stage: &StageSummary) -> Self {
        Self {
            status: stage.status.clone(),
            blocker_state: stage
                .completion_blocker
                .as_ref()
                .map(|blocker| blocker.state),
            blocker_fingerprint: stage
                .completion_blocker
                .as_ref()
                .map(|blocker| blocker.fingerprint.clone()),
        }
    }
}

fn blocker_changed(
    previous: Option<&StageActivityState>,
    blocker: Option<&CompletionBlockerSummary>,
) -> bool {
    blocker.is_some_and(|blocker| {
        previous.is_none_or(|previous| {
            previous.blocker_state != Some(blocker.state)
                || previous.blocker_fingerprint.as_deref() != Some(blocker.fingerprint.as_str())
        })
    })
}

fn blocker_entry(
    stage: &StageSummary,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Option<TuiActivityEntry> {
    let blocker = stage.completion_blocker.as_ref()?;
    let (icon, style) = match blocker.state {
        CompletionBlockerState::Pending => (StageStatus::Executing.icon(), Theme::status_warning()),
        CompletionBlockerState::Blocked | CompletionBlockerState::OwnershipUnknown => {
            (StageStatus::Blocked.icon(), Theme::status_blocked())
        }
    };
    Some(TuiActivityEntry {
        timestamp,
        icon,
        message: format!("{} {}", stage.id, blocker.activity_text()),
        style,
    })
}

fn status_entry(
    stage: &StageSummary,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Option<TuiActivityEntry> {
    let message = match &stage.status {
        StageStatus::Executing => "started",
        StageStatus::Completed => "completed",
        StageStatus::Blocked => "blocked",
        StageStatus::Queued => "ready",
        StageStatus::NeedsHandoff => "needs handoff",
        _ => return None,
    };
    Some(TuiActivityEntry {
        timestamp,
        icon: stage.status.icon(),
        message: format!("{} {message}", stage.id),
        style: stage.status.tui_style(),
    })
}

impl Default for TuiActivityLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
