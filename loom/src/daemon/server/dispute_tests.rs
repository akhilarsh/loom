use super::*;
use crate::fs::work_dir::WorkDir;
use crate::models::stage::{StageStatus, SuccessCriteria, WiringCheck, WiringTest};
use crate::plan::schema::AcceptanceCriterion;
use crate::verify::transitions::save_stage;
use tempfile::TempDir;

fn setup(stage_status: StageStatus, acceptance_len: usize) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let wd = WorkDir::new(tmp.path()).unwrap();
    wd.initialize().unwrap();
    let work_path = wd.root().to_path_buf();
    let mut stage = Stage {
        id: "stage-disp".to_string(),
        name: "Disp".to_string(),
        status: stage_status,
        ..Stage::default()
    };
    for i in 0..acceptance_len {
        stage
            .acceptance
            .push(AcceptanceCriterion::Simple(format!("echo {i}")));
    }
    save_stage(&stage, &work_path).unwrap();
    (tmp, work_path)
}

/// File an acceptance-criterion dispute with no evidence commit or output.
fn file_acceptance(work_dir: &Path, stage_id: &str, index: usize, reason: &str) -> Response {
    handle_dispute_criteria(
        work_dir,
        stage_id,
        CriterionField::Acceptance,
        index,
        reason.to_string(),
        None,
        None,
    )
    .unwrap()
}

/// Give `stage-disp` two wiring checks and two wiring tests.
fn add_wiring_entries(work_dir: &Path) {
    update_stage("stage-disp", work_dir, |stage| {
        for i in 0..2 {
            stage.wiring.push(WiringCheck {
                source: "src/lib.rs".to_string(),
                pattern: format!("pub fn f{i}"),
                description: format!("f{i} is exported"),
                literal: true,
            });
            stage.wiring_tests.push(WiringTest {
                name: format!("t{i}"),
                command: "true".to_string(),
                success_criteria: SuccessCriteria::default(),
                description: None,
            });
        }
        Ok(())
    })
    .unwrap();
}

/// The `field:` line of the one `request.md` filed as `disputes/stage-disp/1`.
fn filed_field(work_dir: &Path) -> String {
    let path = work_dir.join("disputes/stage-disp/1/request.md");
    let content = std::fs::read_to_string(path).unwrap();
    let frontmatter = content.split("---").nth(1).unwrap();
    let yaml: serde_yaml::Value = serde_yaml::from_str(frontmatter).unwrap();
    yaml["field"].as_str().unwrap().to_string()
}

fn refusal_message(response: Response) -> String {
    match response {
        Response::Error { message } => message,
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn wiring_index_is_checked_against_the_wiring_list() {
    // Five acceptance criteria but two wiring checks: index 3 is valid only
    // for the acceptance list.
    let (_tmp, work_dir) = setup(StageStatus::Executing, 5);
    add_wiring_entries(&work_dir);
    let dispute = |index| {
        handle_dispute_criteria(
            &work_dir,
            "stage-disp",
            CriterionField::Wiring,
            index,
            "wrong check".to_string(),
            None,
            None,
        )
        .unwrap()
    };

    let message = refusal_message(dispute(3));
    assert!(message.contains("out of range"), "msg: {message}");
    assert!(message.contains("--field wiring"), "msg: {message}");
    assert!(!work_dir.join("disputes/stage-disp/1").exists());

    assert!(matches!(dispute(1), Response::DisputeCreated { id: 1 }));
    assert_eq!(filed_field(&work_dir), "wiring");
}

#[test]
fn wiring_tests_index_is_checked_against_the_wiring_tests_list() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 5);
    add_wiring_entries(&work_dir);
    let dispute = |index| {
        handle_dispute_criteria(
            &work_dir,
            "stage-disp",
            CriterionField::WiringTests,
            index,
            "wrong test".to_string(),
            None,
            None,
        )
        .unwrap()
    };

    let message = refusal_message(dispute(2));
    assert!(message.contains("out of range"), "msg: {message}");
    assert!(message.contains("--field wiring-tests"), "msg: {message}");
    assert!(!work_dir.join("disputes/stage-disp/1").exists());

    assert!(matches!(dispute(0), Response::DisputeCreated { id: 1 }));
    assert_eq!(filed_field(&work_dir), "wiring-tests");
}

#[test]
fn a_refused_transition_writes_no_request_and_spends_no_budget() {
    let (_tmp, work_dir) = setup(StageStatus::Completed, 1);

    let message = refusal_message(file_acceptance(&work_dir, "stage-disp", 0, "x"));

    assert!(message.contains("cannot dispute stage"), "msg: {message}");
    assert!(!work_dir.join("disputes/stage-disp/1").exists());
    let stage = crate::verify::transitions::load_stage("stage-disp", &work_dir).unwrap();
    assert_eq!(stage.dispute_count, 0);
    assert_eq!(stage.status, StageStatus::Completed);
}

#[test]
fn dispute_persists_request_md_in_per_id_directory() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 3);
    let resp = handle_dispute_criteria(
        &work_dir,
        "stage-disp",
        CriterionField::Acceptance,
        1,
        "bad criterion".to_string(),
        None,
        None,
    )
    .unwrap();
    match resp {
        Response::DisputeCreated { id } => {
            let path = work_dir
                .join("disputes/stage-disp")
                .join(id.to_string())
                .join("request.md");
            assert!(path.exists(), "request.md missing at {}", path.display());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn dispute_does_not_create_verdict_md_or_applied_marker() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 1);
    file_acceptance(&work_dir, "stage-disp", 0, "x");
    let dispute_dir = work_dir.join("disputes/stage-disp/1");
    assert!(!dispute_dir.join("verdict.md").exists());
    assert!(!dispute_dir.join("applied.marker").exists());
}

#[test]
fn dispute_transitions_to_needs_adjudication() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 1);
    file_acceptance(&work_dir, "stage-disp", 0, "x");
    let stage = crate::verify::transitions::load_stage("stage-disp", &work_dir).unwrap();
    assert_eq!(stage.status, StageStatus::NeedsAdjudication);
}

#[test]
fn dispute_rejects_when_budget_exhausted() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 1);
    update_stage("stage-disp", &work_dir, |stage| {
        stage.dispute_count = 3;
        Ok(())
    })
    .unwrap();
    let resp = file_acceptance(&work_dir, "stage-disp", 0, "x");
    match resp {
        Response::Error { message } => assert!(message.contains("budget"), "msg: {message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    // Budget-exhausted MUST escalate the stage to NeedsHumanReview so
    // the agent does not loop futilely. (The state-machine allows the
    // direct Executing → NeedsHumanReview transition used here.)
    let after = crate::verify::transitions::load_stage("stage-disp", &work_dir).unwrap();
    assert_eq!(
        after.status,
        StageStatus::NeedsHumanReview,
        "stage must escalate to NeedsHumanReview on budget exhaustion"
    );
    assert!(
        after
            .review_reason
            .as_deref()
            .unwrap_or("")
            .contains("Dispute budget exhausted"),
        "review_reason should mention budget exhaustion; got: {:?}",
        after.review_reason,
    );
}

#[test]
fn dispute_rejects_invalid_criterion_index() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 2);
    let resp = file_acceptance(&work_dir, "stage-disp", 99, "x");
    match resp {
        Response::Error { message } => {
            assert!(message.contains("out of range"), "msg: {message}")
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn dispute_with_failure_output_truncates_at_4kb() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 1);
    let big = "a".repeat(10_000);
    let resp = handle_dispute_criteria(
        &work_dir,
        "stage-disp",
        CriterionField::Acceptance,
        0,
        "x".to_string(),
        None,
        Some(big),
    )
    .unwrap();
    let id = match resp {
        Response::DisputeCreated { id } => id,
        other => panic!("got {other:?}"),
    };
    let path = work_dir.join(format!("disputes/stage-disp/{id}/request.md"));
    let content = std::fs::read_to_string(&path).unwrap();
    // The frontmatter contains the failure_output; parse YAML and
    // check the field length is bounded.
    let yaml_chunk = content.split("---").nth(1).unwrap();
    let parsed: serde_yaml::Value = serde_yaml::from_str(yaml_chunk).unwrap();
    let fo = parsed["failure_output"].as_str().unwrap();
    assert!(fo.len() <= 4096, "got {}", fo.len());
}

#[test]
fn dispute_works_from_worktree_with_symlinked_work() {
    // Simulate worktree: parent dir holds real .loom/work, worktree dir
    // holds a .loom/work symlink. `.loom` itself stays a real directory
    // on both sides — only the `work` child is a symlink.
    let tmp = TempDir::new().unwrap();
    let main_repo = tmp.path().join("main");
    let worktree = tmp.path().join("worktree");
    std::fs::create_dir_all(&main_repo).unwrap();
    std::fs::create_dir_all(worktree.join(".loom")).unwrap();
    let real_work = main_repo.join(".loom").join("work");
    let wd = WorkDir::new(&main_repo).unwrap();
    wd.initialize().unwrap();
    // Create symlink worktree/.loom/work -> main/.loom/work
    std::os::unix::fs::symlink(&real_work, worktree.join(".loom").join("work")).unwrap();

    let mut stage = Stage {
        id: "stage-sym".to_string(),
        name: "S".to_string(),
        status: StageStatus::Executing,
        ..Stage::default()
    };
    stage
        .acceptance
        .push(AcceptanceCriterion::Simple("x".to_string()));
    save_stage(&stage, &real_work).unwrap();

    let symlinked_work = worktree.join(".loom").join("work");
    let resp = file_acceptance(&symlinked_work, "stage-sym", 0, "y");
    match resp {
        Response::DisputeCreated { id } => {
            // The request.md must land in the REAL .loom/work, not the symlink.
            assert!(real_work
                .join(format!("disputes/stage-sym/{id}/request.md"))
                .exists());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn dispute_rejects_path_traversal_in_stage_id() {
    // SEC: handle_dispute_criteria takes stage_id straight from the wire
    // and otherwise feeds it to create_dir_all / safe_fs. validate_id must
    // reject path-traversal shapes BEFORE any FS write happens.
    let (_tmp, work_dir) = setup(StageStatus::Executing, 1);
    let evil = "../../tmp/escape";
    let resp = file_acceptance(&work_dir, evil, 0, "x");
    match resp {
        Response::Error { message } => {
            assert!(
                message.contains("invalid stage_id"),
                "expected invalid stage_id error, got: {message}",
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }
    // Side-effect check: no escape-attempt directory should exist anywhere
    // under disputes_root.
    let disputes_root = work_dir.join("disputes");
    if disputes_root.exists() {
        for entry in std::fs::read_dir(&disputes_root).unwrap() {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            assert!(
                !name.contains("..") && !name.contains('/'),
                "found suspicious entry under disputes/: {name}",
            );
        }
    }
}

#[test]
fn concurrent_disputes_allocate_distinct_ids_under_flock() {
    let (_tmp, work_dir) = setup(StageStatus::Executing, 5);
    let r1 = file_acceptance(&work_dir, "stage-disp", 0, "a");
    // Model the orchestrator's accept-verdict path before filing again.
    update_stage("stage-disp", &work_dir, |stage| {
        stage.status = StageStatus::Executing;
        Ok(())
    })
    .unwrap();

    let r2 = file_acceptance(&work_dir, "stage-disp", 1, "b");
    let id1 = match r1 {
        Response::DisputeCreated { id } => id,
        other => panic!("{other:?}"),
    };
    let id2 = match r2 {
        Response::DisputeCreated { id } => id,
        other => panic!("{other:?}"),
    };
    assert_ne!(id1, id2, "ids must be distinct");
    assert_eq!(id2, id1 + 1, "second id must be sequential");
}
