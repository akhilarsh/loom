//! The operator-facing text of hold reasons and alerts.

use super::*;

#[test]
fn a_path_with_a_newline_or_an_escape_stays_on_one_line() {
    let paths = vec![
        ".claude/a\nb".to_string(),
        ".claude/\u{1b}[31mred".to_string(),
    ];

    let text = HoldReason::ControlPaths { paths }.to_string();

    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(!text.chars().any(char::is_control), "{text}");
    assert!(
        text.contains(r".claude/a\nb") && text.contains(r".claude/\u{1b}[31mred"),
        "{text}"
    );
}

#[test]
fn an_unattested_reason_names_its_range_and_paths() {
    let reason = HoldReason::Unattested {
        from: "1a2b3c4d".repeat(5),
        to: "5d6e7f80".repeat(5),
        paths: vec!["src/x.rs".to_string()],
    };

    assert_eq!(
        reason.to_string(),
        "unattested move 1a2b3c4d1a2b..5d6e7f805d6e touching src/x.rs"
    );
}

#[test]
fn stage_work_and_unevaluable_reasons_explain_themselves() {
    let branches = vec!["loom/a".to_string(), "loom/b".to_string()];

    assert_eq!(
        HoldReason::StageWork { branches }.to_string(),
        "carries work of loom/a, loom/b"
    );
    let error = "target-guard.json does not parse".to_string();
    assert_eq!(
        HoldReason::Unevaluable { error }.to_string(),
        "could not evaluate: target-guard.json does not parse"
    );
}

#[test]
fn an_alert_for_an_unreadable_record_names_the_accepted_tip_unknown() {
    let error = "target-guard.json does not parse".to_string();
    let hold = Hold {
        accepted: String::new(),
        observed: "5d6e7f80".repeat(5),
        reasons: vec![HoldReason::Unevaluable { error }],
        since: Utc::now(),
    };

    let alert = hold_alert("main", &hold);

    assert!(
        alert.starts_with("Target main held: moved outside loom unknown→5d6e7f805d6e ("),
        "{alert}"
    );
}
