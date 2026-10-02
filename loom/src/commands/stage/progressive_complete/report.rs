//! Operator-facing reports for a merge that stashed changes, was blocked, or
//! was held by the control-path gate.

use anyhow::Result;
use std::path::Path;

use crate::git::merge::{MergeBlock, StashReapply};
use crate::verify::transitions::update_stage;

/// Tell the operator where their uncommitted tracked changes are when the
/// merge stashed them. Changes that were not reapplied are a warning: the
/// merge did land.
pub(in crate::commands::stage) fn report_stash(stash: Option<&StashReapply>) {
    match stash {
        Some(stash) if stash.restored => println!("  {}", stash.notice()),
        Some(stash) => eprintln!("  WARNING: {}", stash.notice()),
        None => {}
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
