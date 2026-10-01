use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::fs::session_files::load_session_exact;
use crate::git::merge::{ActiveMergeState, InProgressMerge};
use crate::models::session::{Session, SessionType};
use crate::models::stage::Stage;
use crate::process::is_process_alive;

use super::helpers;
use super::types::MergeSignalContent;

/// Generate a signal file for a merge conflict resolution session.
///
/// Unlike regular stage signals that run in worktrees, merge signals direct
/// the session to work in the main repository to resolve merge conflicts.
///
/// `in_progress` describes any active merge state on disk so the signal text
/// can branch between "start a fresh merge" and "continue the existing one".
pub fn generate_merge_signal(
    session: &Session,
    stage: &Stage,
    source_branch: &str,
    target_branch: &str,
    conflicting_files: &[String],
    in_progress: Option<&InProgressMerge>,
    work_dir: &Path,
) -> Result<PathBuf> {
    let content = format_merge_signal_content(
        session,
        stage,
        source_branch,
        target_branch,
        conflicting_files,
        in_progress,
    );
    helpers::write_signal_file(&session.id, &content, work_dir)
}

/// Find a live merge resolver session for the given stage by scanning
/// `.loom/work/signals/` for merge signals.
///
/// For each signal of `stage_id`, loads the session record it names and
/// checks PID liveness. If alive -> returns `Some(session_id)`. If dead (or
/// the record is missing) -> removes the stale signal file and continues
/// scanning. A signal outlives its record in ordinary operation: a failed
/// resolver spawn in `attempt_auto_merge` or `spawn_merge_resolver` leaves
/// its signal with no record, and orphan recovery and `loom sessions kill`
/// remove a record before its signal. A record that exists but cannot be read
/// leaves liveness unknown, so its error is returned.
///
/// A signal that cannot be read is attributed through its filename, which is
/// its session's id: a session record naming another stage, or a session that
/// is not a merge resolver, puts it out of this stage's concern; a merge
/// session of this stage is judged alive or dead like any other. With no
/// readable record, the signal could belong to a live resolver of this stage,
/// so the read error is returned.
///
/// Returns `Ok(None)` if no live merge session exists for the stage.
pub fn find_live_merge_session_for_stage(
    stage_id: &str,
    work_dir: &Path,
) -> Result<Option<String>> {
    let signal_ids = super::crud::list_signals(work_dir)?;
    for signal_id in &signal_ids {
        let (session_id, record) = match read_merge_signal(signal_id, work_dir) {
            Ok(Some(signal)) if signal.stage_id == stage_id => {
                let id = signal.session_id;
                let record = load_session_exact(work_dir, &id).with_context(|| {
                    format!("the session record of merge resolver {id} cannot be read")
                })?;
                (id, record)
            }
            Ok(_) => continue,
            Err(error) => {
                match attribute_unreadable_signal(signal_id, stage_id, work_dir, error)? {
                    Some(record) => (signal_id.clone(), Some(record)),
                    None => continue,
                }
            }
        };

        let alive = record
            .and_then(|session| session.pid)
            .is_some_and(is_process_alive);
        if alive {
            return Ok(Some(session_id));
        }
        // Dead session: clean up the stale signal and keep scanning.
        if let Err(e) = super::crud::remove_signal(signal_id, work_dir) {
            tracing::warn!(
                signal_id = %signal_id,
                error = %e,
                "Failed to remove stale merge signal"
            );
        }
    }
    Ok(None)
}

/// The session record of the unreadable signal `signal_id` when that record
/// is a merge session of `stage_id` (or names no stage), `None` when it
/// belongs elsewhere, and `error` when no readable record attributes the
/// signal at all.
fn attribute_unreadable_signal(
    signal_id: &str,
    stage_id: &str,
    work_dir: &Path,
    error: anyhow::Error,
) -> Result<Option<Session>> {
    let Some(record) = load_session_exact(work_dir, signal_id).ok().flatten() else {
        return Err(error.context(format!(
            "merge signal {signal_id} cannot be read, and no session record attributes it to a \
             stage"
        )));
    };
    let ours = record.session_type == SessionType::Merge
        && record
            .stage_id
            .as_deref()
            .is_none_or(|owner| owner == stage_id);
    Ok(ours.then_some(record))
}

/// Read and parse a merge signal file.
///
/// Returns `None` if the signal file doesn't exist or isn't a merge signal.
pub fn read_merge_signal(session_id: &str, work_dir: &Path) -> Result<Option<MergeSignalContent>> {
    let signal_path = work_dir.join("signals").join(format!("{session_id}.md"));

    if !signal_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&signal_path).context("Failed to read signal file")?;

    // Check if this is a merge signal by looking for the merge-specific header
    if !content.contains("# Merge Signal:") {
        return Ok(None);
    }

    let parsed = parse_merge_signal_content(session_id, &content)?;
    Ok(Some(parsed))
}

pub(super) fn format_merge_signal_content(
    session: &Session,
    stage: &Stage,
    source_branch: &str,
    target_branch: &str,
    conflicting_files: &[String],
    in_progress: Option<&InProgressMerge>,
) -> String {
    let mut content = String::new();

    content.push_str(&format!("# Merge Signal: {}\n\n", session.id));

    // Merge context - explain the situation
    content.push_str("## Merge Context\n\n");
    content.push_str("You are resolving a **merge conflict** in the main repository.\n\n");
    content.push_str("- This is NOT a regular stage execution - you are fixing conflicts\n");
    content.push_str("- Work directly in the main repository (not a worktree)\n");
    content.push_str("- Follow the merge instructions below carefully\n\n");

    // Execution rules for merge sessions
    content.push_str(&helpers::format_execution_rules_section("BOTH branches"));

    // Target information
    content.push_str(&helpers::format_target_section(
        &session.id,
        &stage.id,
        Some(source_branch),
        target_branch,
    ));

    // Stage context (if available)
    content.push_str(&helpers::format_stage_context_section(stage));

    // Conflicting files
    content.push_str(&helpers::format_conflicting_files_section(
        conflicting_files,
    ));

    // Task instructions — branch by in-progress merge state.
    content.push_str("## Your Task\n\n");
    match in_progress.map(|m| (&m.state, m.location_path().display().to_string())) {
        None => {
            content.push_str(&format!(
                "1. Run: `git merge {source_branch}` (if not already in merge state)\n"
            ));
            content.push_str("2. Resolve conflicts in the files listed above\n");
            content.push_str("3. Stage resolved files: `git add <resolved-files>`\n");
            content.push_str("4. Review changes and complete the merge: `git commit`\n");
            content.push_str(&format!(
                "5. Run: `loom stage merge {} --resolved`\n\n",
                stage.id
            ));
        }
        Some((ActiveMergeState::HasUnmergedPaths(_), location)) => {
            content.push_str(&format!(
                "A merge is already in progress at `{location}`. \
                 **Do NOT run `git merge` again.**\n\n"
            ));
            content.push_str("1. Resolve conflicts in the files listed above\n");
            content.push_str("2. Stage resolved files: `git add <resolved-files>`\n");
            content.push_str("3. Review changes and complete the merge: `git commit`\n");
            content.push_str(&format!(
                "4. Run: `loom stage merge {} --resolved`\n\n",
                stage.id
            ));
        }
        Some((ActiveMergeState::ResolvedButUncommitted, location)) => {
            content.push_str(&format!(
                "A merge is already in progress at `{location}` with all conflicts resolved.\n\n"
            ));
            content.push_str("1. **Review the staged changes** (`git diff --staged`)\n");
            content.push_str("2. Complete the merge: `git commit`\n");
            content.push_str(&format!(
                "3. Run: `loom stage merge {} --resolved`\n\n",
                stage.id
            ));
        }
    }

    // Important notes
    content.push_str("## Important\n\n");
    content.push_str("- Do NOT modify code beyond what's needed for conflict resolution\n");
    content.push_str("- Preserve intent from BOTH branches where possible\n");
    content.push_str("- If unclear how to resolve, ask the user for guidance\n");
    content.push_str(
        "- The orchestrator removes the stage's worktree and branch once it accepts \
         `--resolved`; do not run `loom worktree remove` yourself\n",
    );

    content.push_str("## Inherited Responsibilities\n\n");
    content.push_str(
        "This resolution session now **owns** this stage. \
         The original execution session has exited.\n\n",
    );
    content.push_str(
        "- The original stage's acceptance criteria already passed before the merge conflict\n",
    );
    content.push_str("- Do NOT re-run the original stage's tasks or acceptance criteria\n");
    content.push_str(&format!(
        "- After resolving: `loom stage merge {} --resolved` marks the stage Completed \
         and triggers dependents\n",
        stage.id
    ));
    content.push_str("- The orchestrator handles plan completion when all stages finish\n");
    content.push_str(
        "- If this session exits without resolving, the orchestrator will spawn a new resolver\n\n",
    );

    content
}

pub(super) fn parse_merge_signal_content(
    session_id: &str,
    content: &str,
) -> Result<MergeSignalContent> {
    let sections = helpers::parse_signal_sections(content);

    // Extract from "Target" section
    let target_lines = sections
        .get("Target")
        .map(|v| v.as_slice())
        .unwrap_or_default();
    let stage_id = helpers::extract_field_from_lines(target_lines, "Stage")
        .unwrap_or_default()
        .to_string();
    let source_branch = helpers::extract_field_from_lines(target_lines, "Source Branch")
        .unwrap_or_default()
        .to_string();
    let target_branch = helpers::extract_field_from_lines(target_lines, "Target Branch")
        .unwrap_or_default()
        .to_string();

    // Extract from "Conflicting Files" section
    let conflict_lines = sections
        .get("Conflicting Files")
        .map(|v| v.as_slice())
        .unwrap_or_default();
    let conflicting_files = helpers::extract_backtick_items(conflict_lines);

    if stage_id.is_empty() {
        bail!("Merge signal file is missing stage_id");
    }

    Ok(MergeSignalContent {
        session_id: session_id.to_string(),
        stage_id,
        source_branch,
        target_branch,
        conflicting_files,
    })
}

#[cfg(test)]
#[path = "merge_liveness_tests.rs"]
mod liveness_tests;
