//! `--eval-edges`: score labelled corpora against the thresholds and render
//! the result. Pure and in memory, so it needs no snapshot and no `.loom/`.

use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use crate::context::eval_edges::{
    corpus_dirs, evaluate_dir, load_thresholds, EdgeQualityReport, Thresholds,
};

/// Failures printed per corpus before `... N more`.
const MAX_FAILURES_SHOWN: usize = 20;

/// Exit code when the directory holds no labelled corpus.
const EXIT_NO_CORPUS: i32 = 2;
/// Exit code when any corpus violates a threshold.
const EXIT_FAILING: i32 = 1;

/// What one `--eval-edges` run prints and the exit code it maps to.
#[derive(Debug)]
pub struct EvalOutcome {
    /// Stdout: the human report or one JSON object.
    pub text: String,
    /// `<n> corpora, <f> failing`; the last line of the human report, and
    /// also the `summary` key of the JSON.
    pub summary: String,
    pub exit_code: i32,
}

/// One corpus's result. A corpus that cannot be evaluated has no report and
/// one violation naming the error.
struct Scored {
    name: String,
    report: Option<EdgeQualityReport>,
    violations: Vec<String>,
}

/// Evaluate the corpus (or corpora) under `dir` against `thresholds`, or the
/// published thresholds when none are given.
pub fn run(dir: &Path, thresholds: Option<&Path>, json: bool) -> Result<EvalOutcome> {
    let thresholds = match thresholds {
        Some(path) => load_thresholds(path)?,
        None => Thresholds::published()?,
    };
    let scored: Vec<Scored> = corpus_dirs(dir)
        .iter()
        .map(|corpus| score_corpus(corpus, &thresholds))
        .collect();
    let failing = scored.iter().filter(|s| !s.violations.is_empty()).count();
    let summary = format!("{} corpora, {failing} failing", scored.len());
    let exit_code = if scored.is_empty() {
        EXIT_NO_CORPUS
    } else if failing > 0 {
        EXIT_FAILING
    } else {
        0
    };
    let text = if json {
        render_json(&scored, &summary)
    } else {
        render_human(&scored, &summary)
    };
    Ok(EvalOutcome {
        text,
        summary,
        exit_code,
    })
}

fn score_corpus(corpus: &Path, thresholds: &Thresholds) -> Scored {
    let name = corpus.file_name().map_or_else(
        || corpus.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    match evaluate_dir(corpus) {
        Ok(report) => Scored {
            name,
            violations: thresholds.check(&report),
            report: Some(report),
        },
        Err(error) => Scored {
            name,
            report: None,
            violations: vec![format!("evaluation failed: {error:#}")],
        },
    }
}

fn render_json(scored: &[Scored], summary: &str) -> String {
    let results: Vec<Value> = scored
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "pass": s.violations.is_empty(),
                "report": s.report,
                "violations": s.violations,
            })
        })
        .collect();
    let failing = scored.iter().filter(|s| !s.violations.is_empty()).count();
    json!({
        "corpora": scored.len(),
        "failing": failing,
        "summary": summary,
        "results": results,
    })
    .to_string()
}

fn render_human(scored: &[Scored], summary: &str) -> String {
    let mut lines = Vec::new();
    for s in scored {
        let verdict = if s.violations.is_empty() {
            "ok"
        } else {
            "FAIL"
        };
        match &s.report {
            Some(report) => {
                lines.push(format!("{}: {} {verdict}", report.dialect, metrics(report)))
            }
            None => lines.push(format!("{}: {verdict}", s.name)),
        }
        lines.extend(s.violations.iter().map(|v| format!("  violation: {v}")));
        if let Some(report) = &s.report {
            lines.extend(failure_lines(&report.failures));
        }
    }
    lines.push(summary.to_string());
    lines.join("\n")
}

/// Every design 14.2 metric on one line.
fn metrics(report: &EdgeQualityReport) -> String {
    let misses: Vec<String> = report
        .impact_false_negatives
        .iter()
        .map(|(depth, missed)| format!("d{depth}={missed}"))
        .collect();
    format!(
        "declaration_recall={:.3} target_precision={:.3} target_recall={:.3} \
         unresolved_rate={:.3} ambiguous_rate={:.3} false_high_confidence={} \
         impact_false_negatives=[{}]",
        report.declaration_recall,
        report.target_precision,
        report.target_recall,
        report.unresolved_rate,
        report.ambiguous_rate,
        report.false_high_confidence,
        misses.join(" ")
    )
}

fn failure_lines(failures: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = failures
        .iter()
        .take(MAX_FAILURES_SHOWN)
        .map(|f| format!("  miss: {f}"))
        .collect();
    if failures.len() > MAX_FAILURES_SHOWN {
        lines.push(format!(
            "  ... {} more",
            failures.len() - MAX_FAILURES_SHOWN
        ));
    }
    lines
}
