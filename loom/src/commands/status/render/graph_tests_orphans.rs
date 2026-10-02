//! Orphaned-stage warning tests for the static `loom status` tree, split out
//! of `graph_tests.rs` to keep that file under the line-count limit.

use super::{make_stage_summary, make_status_data};
use crate::commands::status::data::ActivityStatus;
use crate::commands::status::render::graph::render_graph;
use crate::models::stage::StageStatus;

#[test]
fn test_orphaned_stage_shows_warning() {
    // A stage whose activity status is Orphaned (claims Executing, no session
    // record) must surface the one-line explanation and both ways out.
    let mut stage = make_stage_summary("my-stage", vec![], StageStatus::Executing);
    stage.activity_status = ActivityStatus::Orphaned;

    let data = make_status_data(vec![stage]);
    let mut output = Vec::new();
    render_graph(&mut output, &data).unwrap();
    let output_str = String::from_utf8(output).unwrap();

    assert!(
        output_str.contains("claims Executing with no session record"),
        "Expected orphaned warning text in output"
    );
    assert!(
        output_str.contains("loom repair"),
        "Expected repair hint in output"
    );
    assert!(
        output_str.contains("loom stage reset --kill-session my-stage"),
        "Expected reset hint in output"
    );
}

#[test]
fn test_non_orphaned_stage_has_no_orphaned_warning() {
    // A normally-executing stage (Working activity status) must not show the
    // orphaned warning text.
    let mut stage = make_stage_summary("my-stage", vec![], StageStatus::Executing);
    stage.activity_status = ActivityStatus::Working;

    let data = make_status_data(vec![stage]);
    let mut output = Vec::new();
    render_graph(&mut output, &data).unwrap();
    let output_str = String::from_utf8(output).unwrap();

    assert!(
        !output_str.contains("claims Executing with no session record"),
        "Should not show orphaned warning when activity status is not Orphaned"
    );
}
