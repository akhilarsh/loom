//! In-family codex model fallback for the pressure pipeline.
//!
//! A newly announced codex model is often not yet enabled for every account;
//! codex then exits non-zero with a "model is not supported" error before any
//! review happens. The runner here waits on the background codex process on
//! its own thread and, when an attempt fails that way, re-runs codex with the
//! next model from [`crate::codex::codex_model_candidates`]. The runner never
//! prints: the foreground Claude TUI owns the terminal until it exits, so the
//! fallbacks taken are handed back as notes and printed afterwards.

use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::spawn::spawn_codex_background;
use crate::codex::codex_model_candidates;

/// Phrases codex and the API print when the account cannot use a model,
/// matched case-insensitively.
const UNAVAILABLE_PHRASES: [&str; 4] = [
    "model is not supported",
    "model not found",
    "model_not_found",
    "does not exist",
];

/// The first log line showing the account cannot use `model`: a codex error
/// line (`ERROR: ...`) that names `model` together with an unavailability
/// phrase. The log also holds codex's tool output and model prose, which may
/// quote these phrases (for example a `cat` of this file's test strings), so
/// only `ERROR` lines count; otherwise an unrelated non-zero exit would
/// trigger a retry. Requiring the model and phrase on one line keeps an
/// unrelated error — or codex's harmless `Model metadata for ... not found`
/// warning — from triggering a fallback. The line itself is returned so the
/// fallback note can carry it.
fn unavailable_line<'a>(log: &'a str, model: &str) -> Option<&'a str> {
    log.lines().find(|line| {
        let lower = line.to_lowercase();
        line.trim_start().starts_with("ERROR")
            && line.contains(model)
            && UNAVAILABLE_PHRASES.iter().any(|p| lower.contains(p))
    })
}

/// What the codex runner reports once its last attempt has exited.
pub(super) struct CodexOutcome {
    /// Exit status of the last attempt.
    pub(super) status: ExitStatus,
    /// Model the last attempt ran with.
    pub(super) model_used: String,
    /// One note per fallback taken, each carrying the codex error line that
    /// caused it, since the next attempt re-creates the log.
    pub(super) notes: Vec<String>,
    /// Every model attempted, in order.
    pub(super) tried: Vec<String>,
    /// The last attempt also failed as unavailable and no candidate remained.
    pub(super) exhausted: bool,
}

impl CodexOutcome {
    /// Print each fallback taken and, when no candidate was available, one
    /// line naming every model tried. Called only after Claude has exited.
    pub(super) fn print_fallbacks(&self) {
        for note in &self.notes {
            println!("{} {note}", "!".yellow().bold());
        }
        if self.exhausted && self.tried.len() > 1 {
            println!(
                "{} codex: no candidate model is available to this account (tried {})",
                "!".yellow().bold(),
                self.tried.join(", ")
            );
        }
    }
}

/// A background codex run: the runner thread working through the candidates.
pub(super) struct CodexRun {
    handle: JoinHandle<Result<CodexOutcome>>,
}

/// Owned copy of everything one codex attempt needs, moved into the runner
/// thread so it outlives the caller's borrows.
struct CodexJob {
    codex_path: PathBuf,
    repo_root: PathBuf,
    skill: String,
    log_path: PathBuf,
    effort: String,
}

impl CodexJob {
    /// Spawn one attempt with `model`; the log file is re-created.
    fn spawn(&self, model: &str) -> Result<Child> {
        spawn_codex_background(
            &self.codex_path,
            &self.repo_root,
            &self.skill,
            &self.log_path,
            model,
            &self.effort,
        )
    }

    /// The log line showing `model` is unavailable, if the last attempt's log
    /// has one. An unreadable log counts as "no", so the failure is reported
    /// as a plain codex failure instead of being retried blindly.
    fn unavailable_line(&self, model: &str) -> Option<String> {
        let bytes = std::fs::read(&self.log_path).ok()?;
        let log = String::from_utf8_lossy(&bytes);
        unavailable_line(&log, model).map(|line| line.trim().to_string())
    }
}

/// Start codex with the first of `candidates` and hand the rest of the run to
/// a runner thread. The first spawn happens on the caller's thread so a spawn
/// failure errors out before the Claude session starts. The effort is the
/// same for every attempt.
pub(super) fn spawn_codex_with_fallback(
    codex_path: &Path,
    repo_root: &Path,
    skill: &str,
    log_path: &Path,
    candidates: Vec<String>,
    effort: &str,
) -> Result<CodexRun> {
    let job = CodexJob {
        codex_path: codex_path.to_path_buf(),
        repo_root: repo_root.to_path_buf(),
        skill: skill.to_string(),
        log_path: log_path.to_path_buf(),
        effort: effort.to_string(),
    };
    let first = candidates.first().context("no codex model to run")?;
    let child = job.spawn(first)?;
    let handle = thread::spawn(move || run_attempts(&job, child, &candidates));
    Ok(CodexRun { handle })
}

/// Runner-thread body: wait on each attempt, moving to the next candidate
/// only while the failure is the account lacking the model. Never prints.
fn run_attempts(job: &CodexJob, mut child: Child, candidates: &[String]) -> Result<CodexOutcome> {
    let mut notes = Vec::new();
    let mut index = 0;
    loop {
        let model = &candidates[index];
        let status = child.wait().context("failed to wait for codex")?;
        let unavailable = if status.success() {
            None
        } else {
            job.unavailable_line(model)
        };
        match (unavailable, candidates.get(index + 1)) {
            (Some(line), Some(next)) => {
                notes.push(fallback_note(model, next, &line));
                child = job
                    .spawn(next)
                    .with_context(|| format!("failed to spawn codex with fallback model {next}"))?;
                index += 1;
            }
            (unavailable, _) => {
                return Ok(CodexOutcome {
                    status,
                    model_used: model.clone(),
                    notes,
                    tried: candidates[..=index].to_vec(),
                    exhausted: unavailable.is_some(),
                });
            }
        }
    }
}

/// The note recorded when `from` was unavailable and `to` runs next, with
/// the codex error line indented beneath it.
fn fallback_note(from: &str, to: &str, error_line: &str) -> String {
    format!(
        "codex model {from} is unavailable to this account — fell back to {to}\n    {error_line}"
    )
}

/// Wait for the codex runner, showing a small spinner while it is still
/// running after the foreground Claude session has ended. A panicked runner
/// thread becomes an error.
pub(super) fn wait_codex(run: CodexRun, log_path: &Path) -> Result<CodexOutcome> {
    const FRAMES: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];
    let mut i = 0usize;
    while !run.handle.is_finished() {
        print!(
            "\r{} waiting for codex review… (output → {})",
            FRAMES[i % FRAMES.len()],
            log_path.display()
        );
        let _ = std::io::stdout().flush();
        i += 1;
        thread::sleep(Duration::from_millis(200));
    }
    // Clear the spinner line.
    print!("\r\x1b[K");
    let _ = std::io::stdout().flush();
    run.handle
        .join()
        .map_err(|_| anyhow!("codex runner thread panicked"))?
}

/// The codex slot as shown in the run header and dry-run preview: the model,
/// plus its fallback chain when it has one, e.g.
/// `gpt-6.1-sol (fallback: gpt-6-sol)`.
pub(super) fn codex_model_label(model: &str) -> String {
    match codex_model_candidates(model).split_first() {
        Some((first, rest)) if !rest.is_empty() => {
            format!("{first} (fallback: {})", rest.join(", "))
        }
        _ => model.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNSUPPORTED_400: &str = r#"ERROR: {"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-9-nonexistent' model is not supported when using Codex with a ChatGPT account."}}"#;

    /// Whether `log` shows `model` unavailable — the runner's fallback test.
    fn model_unavailable(log: &str, model: &str) -> bool {
        unavailable_line(log, model).is_some()
    }

    #[test]
    fn model_unavailable_matches_the_captured_400_line() {
        let log = format!("starting codex\n{UNSUPPORTED_400}\n");
        assert_eq!(
            unavailable_line(&log, "gpt-9-nonexistent"),
            Some(UNSUPPORTED_400)
        );
    }

    #[test]
    fn model_unavailable_matches_the_404_form() {
        assert!(model_unavailable(
            "ERROR: unexpected status 404 Not Found: Model not found gpt-6.1-sol",
            "gpt-6.1-sol"
        ));
    }

    #[test]
    fn model_unavailable_ignores_quoted_text() {
        let quoted = "    \"404 Not Found: Model not found gpt-6.1-sol\",\n\
                      gpt-6.1-sol does not exist on some accounts";
        assert!(!model_unavailable(quoted, "gpt-6.1-sol"));
    }

    #[test]
    fn model_unavailable_ignores_the_metadata_warning() {
        assert!(!model_unavailable(
            "warning: Model metadata for `gpt-6.1-sol` not found",
            "gpt-6.1-sol"
        ));
    }

    #[test]
    fn model_unavailable_requires_the_model_on_the_same_line() {
        assert!(!model_unavailable(UNSUPPORTED_400, "gpt-6.1-sol"));
        let split = "running gpt-6.1-sol\nerror: the model is not supported";
        assert!(!model_unavailable(split, "gpt-6.1-sol"));
    }

    #[test]
    fn model_unavailable_is_false_for_an_empty_log() {
        assert!(!model_unavailable("", "gpt-6.1-sol"));
    }

    #[test]
    fn model_unavailable_line_is_what_the_fallback_note_carries() {
        let line = unavailable_line(UNSUPPORTED_400, "gpt-9-nonexistent").unwrap();
        let note = fallback_note("gpt-9-nonexistent", "gpt-9-other", line);
        assert!(note.starts_with(
            "codex model gpt-9-nonexistent is unavailable to this account — fell back to gpt-9-other\n    "
        ));
        assert!(note.ends_with("with a ChatGPT account.\"}}"));
    }

    #[test]
    fn codex_model_label_shows_the_chain_only_when_there_is_one() {
        assert_eq!(
            codex_model_label("gpt-6.1-sol"),
            "gpt-6.1-sol (fallback: gpt-6-sol)"
        );
        assert_eq!(codex_model_label("gpt-6-astra"), "gpt-6-astra");
    }
}
