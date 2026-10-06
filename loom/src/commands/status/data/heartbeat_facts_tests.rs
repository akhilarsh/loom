//! When `loom status` reports a stage as stalled past its stall recoveries.

use super::stalled_after_recoveries;
use crate::models::stage::{Stage, StageStatus, StallExhaustion};

const SILENT_SECS: u64 = 903;

/// An `Executing` stage on session `s-3` that loom stopped recovering after
/// two stalls, when `s-3` had been silent `SILENT_SECS`.
fn left_for_operator() -> Stage {
    Stage {
        id: "s1".to_string(),
        status: StageStatus::Executing,
        session: Some("s-3".to_string()),
        stall_recoveries: 2,
        stall_exhausted: Some(StallExhaustion {
            session_id: "s-3".to_string(),
            silent_secs: SILENT_SECS,
        }),
        ..Stage::default()
    }
}

#[test]
fn a_session_still_silent_since_loom_gave_up_is_stalled() {
    let stage = left_for_operator();

    assert_eq!(stalled_after_recoveries(&stage, Some(SILENT_SECS)), Some(2));
    assert_eq!(
        stalled_after_recoveries(&stage, Some(SILENT_SECS + 600)),
        Some(2)
    );
}

#[test]
fn a_session_that_answered_since_is_not_stalled() {
    let stage = left_for_operator();

    assert_eq!(
        stalled_after_recoveries(&stage, Some(SILENT_SECS - 1)),
        None
    );
    assert_eq!(stalled_after_recoveries(&stage, None), None);
}

#[test]
fn a_reset_or_a_successor_session_retires_the_record() {
    let mut successor = left_for_operator();
    successor.session = Some("s-4".to_string());
    let mut reset = left_for_operator();
    reset.status = StageStatus::WaitingForDeps;
    reset.session = None;

    assert_eq!(
        stalled_after_recoveries(&successor, Some(SILENT_SECS)),
        None
    );
    assert_eq!(stalled_after_recoveries(&reset, Some(SILENT_SECS)), None);
}

#[test]
fn a_stage_never_left_for_an_operator_is_not_stalled() {
    let stage = Stage {
        stall_exhausted: None,
        ..left_for_operator()
    };

    assert_eq!(stalled_after_recoveries(&stage, Some(SILENT_SECS)), None);
}
