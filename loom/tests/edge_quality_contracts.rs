//! Contracts for stage edge-quality-eval: the labelled-corpus evaluator and its thresholds.
//!
//! Each test pins one rule of `doc/plans/briefs/source-graph-mechanism/design.md`
//! section 14. The surface is `loom::context::eval_edges::{evaluate_dir,
//! load_thresholds, EdgeQualityReport, Thresholds}`; `Thresholds` derives
//! `Debug` and `PartialEq`, and the thresholds file is a flat YAML mapping of the
//! five design 14.2 keys.

use std::fs;
use std::path::{Path, PathBuf};

use loom::context::eval_edges::{evaluate_dir, load_thresholds, EdgeQualityReport, Thresholds};
use loom::context::extract::dialect::DIALECTS;
use tempfile::TempDir;

/// The design 14.2 thresholds, written before any corpus is examined.
const DESIGN_THRESHOLDS: &str = "declaration_recall: 1.0\n\
false_high_confidence: 0\n\
target_precision: 0.95\n\
target_recall: 0.5\n\
impact_false_negatives_depth_1: 0\n";

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn published_thresholds() -> Thresholds {
    let path = package_root().join("eval/edge-quality-thresholds.yaml");
    load_thresholds(&path).unwrap_or_else(|e| panic!("load {}: {e:#}", path.display()))
}

/// A TempDir Rust corpus: `src/lib.rs` plus `labels.yaml`.
fn rust_corpus(lib_rs: &str, labels_yaml: &str) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir src");
    fs::write(dir.path().join("src/lib.rs"), lib_rs).expect("write lib.rs");
    fs::write(dir.path().join("labels.yaml"), labels_yaml).expect("write labels.yaml");
    dir
}

fn evaluate(dir: &Path) -> EdgeQualityReport {
    evaluate_dir(dir).unwrap_or_else(|e| panic!("evaluate_dir {}: {e:#}", dir.display()))
}

#[test]
fn false_high_confidence_is_counted() {
    let corpus = rust_corpus(
        "fn helper() {}\nfn other() {}\nfn run() {\n    helper();\n}\n",
        "dialect: rust\n\
declarations: []\n\
references:\n\
  - {path: src/lib.rs, line: 4, symbol: helper, expect: {target: \"src/lib.rs#function:other\"}}\n\
impact: []\n",
    );

    let report = evaluate(corpus.path());

    assert_eq!(report.dialect, "rust", "report: {report:#?}");
    assert_eq!(
        report.false_high_confidence, 1,
        "the 0.8 local-name edge to helper contradicts the label expecting other; report: {report:#?}"
    );
    assert!(
        !report.failures.is_empty(),
        "a wrong bound target is a miss; report: {report:#?}"
    );
}

#[test]
fn impact_false_negatives_are_counted_by_depth() {
    let corpus = rust_corpus(
        "fn helper() {}\nfn run() {\n    helper();\n}\nfn lonely() {}\n",
        "dialect: rust\n\
declarations: []\n\
references: []\n\
impact:\n\
  - {start: \"src/lib.rs#function:helper\", depth: 1, expect: [\"src/lib.rs#function:run\", \"src/lib.rs#function:lonely\"]}\n",
    );

    let report = evaluate(corpus.path());

    assert_eq!(
        report.impact_false_negatives.get(&1),
        Some(&1),
        "run is reached at depth 1, lonely is not; report: {report:#?}"
    );
}

#[test]
fn every_dialect_meets_published_thresholds() {
    let dir = TempDir::new().expect("tempdir");
    let design_path = dir.path().join("design-thresholds.yaml");
    fs::write(&design_path, DESIGN_THRESHOLDS).expect("write design thresholds");
    let design = load_thresholds(&design_path).expect("load design thresholds");
    let thresholds = published_thresholds();
    assert_eq!(
        thresholds, design,
        "eval/edge-quality-thresholds.yaml must equal design 14.2 exactly"
    );

    let labeled = package_root().join("tests/fixtures/source/labeled");
    let mut problems = Vec::new();
    for spec in DIALECTS.iter().filter(|spec| spec.pack.compiled()) {
        let corpus = labeled.join(spec.id);
        if !corpus.join("labels.yaml").is_file() {
            problems.push(format!(
                "{}: no labelled corpus at {}",
                spec.id,
                corpus.display()
            ));
            continue;
        }
        let report = match evaluate_dir(&corpus) {
            Ok(report) => report,
            Err(e) => {
                problems.push(format!("{}: evaluate_dir failed: {e:#}", spec.id));
                continue;
            }
        };
        if report.dialect != spec.id {
            problems.push(format!(
                "{}: report names dialect {}",
                spec.id, report.dialect
            ));
        }
        for violation in thresholds.check(&report) {
            problems.push(format!("{}: {violation}", spec.id));
        }
    }

    assert!(
        problems.is_empty(),
        "threshold failures:\n{}",
        problems.join("\n")
    );
}

#[test]
fn thin_corpus_fails_thresholds() {
    let corpus = rust_corpus(
        "fn a() {}\n",
        "dialect: rust\ndeclarations: []\nreferences: []\nimpact: []\n",
    );

    let report = evaluate(corpus.path());
    let violations = published_thresholds().check(&report);

    assert!(
        !violations.is_empty(),
        "a corpus with no labels and no syntax-error file must fail; report: {report:#?}"
    );
}
