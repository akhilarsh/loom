//! Operator-facing reports for a merge that stashed changes, was blocked, or
//! was held by the control-path gate.

use anyhow::Result;
use std::path::Path;

use crate::git::merge::{MergeBlock, StashReapply};
use crate::verify::transitions::update_stage;

/// Tell the operator where their uncommitted tracked changes are when the
/// merge stashed them, and record the outcome on the stage so `loom status`
/// still shows unrestored changes later. Changes that were not reapplied are a
/// warning: the merge did land.
pub(in crate::commands::stage) fn note_merge_stash(
    stage_id: &str,
    work_dir: &Path,
    stash: Option<StashReapply>,
) {
    let Some(stash) = stash else { return };
    if stash.restored {
        println!("  {}", stash.notice());
    } else {
        eprintln!("  WARNING: {}", stash.notice());
    }
    if let Err(error) = update_stage(stage_id, work_dir, |s| {
        s.record_merge_stash(stash);
        Ok(())
    }) {
        eprintln!("  Warning: could not record the stash outcome on stage '{stage_id}': {error:#}");
    }
}

/// Print why the merge is blocked and who retries it.
pub(in crate::commands::stage) fn report_merge_block(stage_id: &str, block: &MergeBlock) {
    println!("  Merge blocked: {block}");
    println!(
        "  A running daemon retries the merge every tick; otherwise run `loom stage merge {stage_id}`"
    );
}

/// Print why the control-path gate held the merge and how an operator
/// releases it.
pub(in crate::commands::stage) fn report_merge_hold(stage_id: &str, reason: &str) {
    println!("  Merge held: {reason}");
    println!(
        "  In .worktrees/{stage_id} review the change, then run \
         `loom stage human-review {stage_id} --force-complete`"
    );
}

/// Route `stage_id` to `NeedsHumanReview` on the fresh on-disk stage and
/// report the hold. For the commands that hold a branch without the progressive
/// merge's captured commit.
pub(in crate::commands::stage) fn route_held_to_review(
    stage_id: &str,
    work_dir: &Path,
    reason: &str,
) -> Result<()> {
    update_stage(stage_id, work_dir, |s| {
        s.route_to_review(reason);
        Ok(())
    })?;
    report_merge_hold(stage_id, reason);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::stage::Stage;
    use crate::verify::transitions::{load_stage, save_stage};

    #[test]
    fn note_merge_stash_records_the_outcome_and_ignores_none() {
        let work_dir = tempfile::TempDir::new().unwrap();
        let stage = Stage {
            id: "s".to_string(),
            ..Stage::default()
        };
        save_stage(&stage, work_dir.path()).unwrap();

        note_merge_stash("s", work_dir.path(), None);
        assert_eq!(load_stage("s", work_dir.path()).unwrap().merge.stash, None);

        let lost = StashReapply {
            backup_ref: "refs/loom/autostash/s-1".to_string(),
            restored: false,
        };
        note_merge_stash("s", work_dir.path(), Some(lost.clone()));
        assert_eq!(
            load_stage("s", work_dir.path()).unwrap().merge.stash,
            Some(lost)
        );
    }
}
