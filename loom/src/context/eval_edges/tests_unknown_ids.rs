//! A labelled id the corpus graph has no node for fails the corpus.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::{corpus_dirs, evaluate_dir, EdgeQualityReport, Thresholds};
use crate::context::extract::dialect::dialect_by_id;

const LIB_RS: &str = "fn helper() {}\nfn other() {}\nfn run() {\n    helper();\n}\n";

/// Score the corpus of `LIB_RS` labelled by `labels`.
fn report_of(labels: &str) -> EdgeQualityReport {
    let dir = TempDir::new().expect("tempdir");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir");
    fs::write(dir.path().join("labels.yaml"), labels).expect("write labels");
    fs::write(dir.path().join("src/lib.rs"), LIB_RS).expect("write lib");
    evaluate_dir(dir.path()).expect("evaluate")
}

/// Assert `report` carries exactly one unknown-id message holding every needle,
/// and that `check` turns it into a violation of the corpus.
fn assert_one_violation(report: &EdgeQualityReport, needles: &[&str]) {
    assert_eq!(report.unknown_label_ids.len(), 1, "{report:#?}");
    let violations = Thresholds::published().expect("thresholds").check(report);
    assert!(
        violations
            .iter()
            .any(|violation| needles.iter().all(|needle| violation.contains(needle))),
        "no violation holds {needles:?}: {violations:#?}"
    );
}

#[test]
fn a_target_id_the_graph_lacks_names_the_label_and_the_id() {
    let report = report_of(
        r#"dialect: rust
references:
  - {path: src/lib.rs, line: 4, symbol: helper, expect: {target: "src/lib.rs#function:helpr"}}
"#,
    );

    assert_one_violation(
        &report,
        &["src/lib.rs:4 helper", "src/lib.rs#function:helpr"],
    );
}

#[test]
fn an_ambiguous_candidate_the_graph_lacks_is_named() {
    let report = report_of(
        r#"dialect: rust
references:
  - {path: src/lib.rs, line: 4, symbol: helper, expect: {ambiguous: ["src/lib.rs#function:helper", "src/lib.rs#function:ghost"]}}
"#,
    );

    assert_one_violation(
        &report,
        &["src/lib.rs:4 helper", "src/lib.rs#function:ghost"],
    );
}

#[test]
fn an_impact_start_and_expected_id_the_graph_lacks_are_named() {
    let report = report_of(
        r#"dialect: rust
impact:
  - {start: "src/lib.rs#function:helpr", depth: 1, expect: ["src/lib.rs#function:run", "src/lib.rs#function:rn"]}
"#,
    );

    assert_eq!(report.unknown_label_ids.len(), 2, "{report:#?}");
    let violations = Thresholds::published().expect("thresholds").check(&report);
    for id in ["src/lib.rs#function:helpr", "src/lib.rs#function:rn"] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("impact") && violation.contains(id)),
            "{id}: {violations:#?}"
        );
    }
}

#[test]
fn a_declaration_path_the_graph_lacks_is_named() {
    let report = report_of(
        r#"dialect: rust
declarations:
  - {path: src/lb.rs, kind: function, scope: [helper], line: 1}
"#,
    );

    assert_one_violation(&report, &["src/lb.rs:1", "helper"]);
}

#[test]
fn external_labels_and_ids_the_graph_has_are_not_reported() {
    let report = report_of(
        r#"dialect: rust
declarations:
  - {path: src/lib.rs, kind: function, scope: [helper], line: 1}
references:
  - {path: src/lib.rs, line: 4, symbol: helper, expect: {target: "src/lib.rs#function:helper"}}
  - {path: src/lib.rs, line: 4, symbol: fetch, expect: {external: true}}
impact:
  - {start: "src/lib.rs#function:helper", depth: 1, expect: ["src/lib.rs#function:run"]}
"#,
    );

    assert!(report.unknown_label_ids.is_empty(), "{report:#?}");
}

#[test]
fn every_shipped_corpus_labels_only_nodes_its_graph_has() {
    let labeled = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/source/labeled");
    let corpora = corpus_dirs(&labeled);
    assert!(!corpora.is_empty(), "no corpus under {}", labeled.display());

    // A corpus whose grammar pack is compiled out extracts file nodes only, so
    // only compiled dialects are checked, as the edge-quality contract does.
    let compiled = corpora.into_iter().filter(|corpus| {
        corpus
            .file_name()
            .and_then(|name| dialect_by_id(&name.to_string_lossy()))
            .is_some_and(|spec| spec.pack.compiled())
    });
    for corpus in compiled {
        let report = evaluate_dir(&corpus).expect("evaluate");

        assert_eq!(
            report.unknown_label_ids,
            Vec::<String>::new(),
            "{}",
            corpus.display()
        );
    }
}
