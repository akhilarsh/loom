//! The typed reason a stage's merge was not advanced (`Stage::merge`)
//! and the transitions that set and clear it.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::types::{Stage, StageStatus};
use crate::git::{MergeBlock, StashReapply};
use crate::models::failure::{FailureInfo, FailureType};

/// A stage's merge state, flattened into the stage file so `merge_block` and
/// `merge_stash` stay top-level keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRecord {
    /// Why the target branch was not advanced; set while `MergeBlocked`.
    #[serde(
        rename = "merge_block",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub block: Option<MergeBlock>,
    /// What the merge did with the operator's stashed changes in the main
    /// checkout; never cleared, it is the stage's merge note.
    #[serde(
        rename = "merge_stash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub stash: Option<StashReapply>,
}

impl Stage {
    /// Record `block` as the reason the merge did not advance and move the
    /// stage to `MergeBlocked`, with the sentence in `failure_info` so
    /// `loom status` shows it. The daemon retries such a stage every tick.
    pub fn block_merge(&mut self, block: MergeBlock) {
        let sentence = block.to_string();
        self.failure_info = Some(FailureInfo {
            failure_type: FailureType::InfrastructureError,
            detected_at: Utc::now(),
            evidence: vec![sentence.clone()],
        });
        if let MergeBlock::StashNotRestored { backup_ref } = &block {
            self.record_merge_stash(StashReapply {
                backup_ref: backup_ref.clone(),
                restored: false,
            });
        }
        self.merge.block = Some(block);
        if self.status != StageStatus::MergeBlocked && self.try_mark_merge_blocked().is_err() {
            self.force_status_with_reason(StageStatus::MergeBlocked, &sentence);
        }
    }

    /// Record what the merge did with the operator's stashed changes. Never
    /// cleared: it stays as the stage's merge note. A restored note does not
    /// replace an unrestored one, whose stash still holds work the checkout
    /// lacks; a new unrestored note replaces any.
    pub fn record_merge_stash(&mut self, stash: StashReapply) {
        let keeps_unrestored = stash.restored
            && self
                .merge
                .stash
                .as_ref()
                .is_some_and(|existing| !existing.restored);
        if !keeps_unrestored {
            self.merge.stash = Some(stash);
        }
    }

    /// Mark the stage as having merge conflicts.
    ///
    /// This sets both the status to MergeConflict and the merge_conflict flag.
    /// The stage work is complete but cannot be merged due to conflicts.
    ///
    /// # Returns
    /// `Ok(())` if the transition succeeded, `Err` if invalid
    pub fn try_mark_merge_conflict(&mut self) -> anyhow::Result<()> {
        self.try_transition(StageStatus::MergeConflict)?;
        self.merge_conflict = true;
        Ok(())
    }

    /// Complete the merge: clear `merge_conflict` and set `merged`. Unless the stage is
    /// already `Completed` (auto-merge disabled leaves it there, and `loom stage merge`
    /// then runs against it), also transition to `Completed` and stamp the timestamps.
    ///
    /// # Returns
    /// `Ok(())` if the transition succeeded, `Err` if invalid
    pub fn try_complete_merge(&mut self) -> anyhow::Result<()> {
        if self.status != StageStatus::Completed {
            self.try_transition(StageStatus::Completed)?;
            self.stamp_completed();
        }
        self.merge_conflict = false;
        self.merged = true;
        Ok(())
    }

    /// Mark the stage as merge blocked (merge failed with actual error, not conflicts).
    ///
    /// This indicates the merge operation failed due to an error (not conflicts).
    /// The stage can be retried by transitioning back to Executing.
    ///
    /// # Returns
    /// `Ok(())` if the transition succeeded, `Err` if invalid
    pub fn try_mark_merge_blocked(&mut self) -> anyhow::Result<()> {
        self.try_transition(StageStatus::MergeBlocked)
    }

    /// Forget the typed block once the stage's status is no longer
    /// `MergeBlocked`; both status writers call this after assigning.
    pub(super) fn drop_merge_block_unless_blocked(&mut self) {
        if self.status != StageStatus::MergeBlocked {
            self.clear_merge_block();
        }
    }

    /// Why a stage whose branch has zero commits beyond `target` goes to human
    /// review instead of merging: a no-op merge would stand for work that was
    /// never committed.
    pub fn zero_commit_reason(stage_id: &str, target: &str) -> String {
        let branch = crate::git::branch::branch_name_for_stage(stage_id);
        format!(
            "branch {branch} has zero commits beyond {target}: the agent never committed work \
             for this stage. Re-queue it with `loom stage human-review {stage_id} --approve`, or \
             redo it manually."
        )
    }

    /// Move the stage to `NeedsHumanReview` with `reason`, whatever its
    /// status, clearing any merge block. The merge gate uses it for a branch
    /// that touches a control path.
    pub fn route_to_review(&mut self, reason: &str) {
        self.clear_merge_block();
        self.force_status_with_reason(StageStatus::NeedsHumanReview, reason);
        self.review_reason = Some(reason.to_string());
    }

    /// Forget the typed merge block. Every transition out of a blocked merge
    /// calls this, so a stale reason never outlives the status it explained.
    /// The `failure_info` that `block_merge` wrote goes with the block; with
    /// no block recorded, an unrelated `failure_info` stays.
    pub fn clear_merge_block(&mut self) {
        if self.merge.block.take().is_some() {
            self.failure_info = None;
        }
    }

    /// Record `commit`, the stage branch head, as `completed_commit` unless one
    /// is already recorded. A stage entering a merge conflict calls this before
    /// any resolver touches the branch, so the stage's own work stays provable.
    pub fn record_completed_commit_if_missing(&mut self, commit: Option<&str>) {
        if self.completed_commit.is_none() {
            self.completed_commit = commit.map(str::to_string);
        }
    }

    /// Move the stage to `MergeConflict`, clearing any merge block.
    pub fn enter_merge_conflict(&mut self) {
        self.clear_merge_block();
        if self.status == StageStatus::MergeConflict {
            self.merge_conflict = true;
        } else if let Err(error) = self.try_mark_merge_conflict() {
            self.force_status_with_reason(
                StageStatus::MergeConflict,
                &format!("the merge conflicts but the transition was illegal: {error}"),
            );
            self.merge_conflict = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_in(status: StageStatus) -> Stage {
        Stage {
            id: "s".to_string(),
            status,
            ..Stage::default()
        }
    }

    #[test]
    fn block_merge_records_the_reason_and_status() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.block_merge(MergeBlock::TargetMoved);
        assert_eq!(stage.status, StageStatus::MergeBlocked);
        assert_eq!(stage.merge.block, Some(MergeBlock::TargetMoved));
        let info = stage.failure_info.expect("failure_info shows the sentence");
        assert_eq!(info.evidence, vec![MergeBlock::TargetMoved.to_string()]);
    }

    #[test]
    fn record_merge_stash_keeps_the_outcome() {
        let mut stage = stage_in(StageStatus::Completed);
        let stash = StashReapply {
            backup_ref: "refs/loom/autostash/s-1".to_string(),
            restored: true,
        };
        stage.record_merge_stash(stash.clone());
        assert_eq!(stage.merge.stash, Some(stash));
    }

    #[test]
    fn block_merge_stash_not_restored_records_the_unrestored_stash() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.block_merge(MergeBlock::StashNotRestored {
            backup_ref: "refs/loom/autostash/s-1".to_string(),
        });
        assert_eq!(
            stage.merge.stash,
            Some(StashReapply {
                backup_ref: "refs/loom/autostash/s-1".to_string(),
                restored: false,
            })
        );
        stage.block_merge(MergeBlock::TargetMoved);
        assert!(stage.merge.stash.is_some(), "a later block keeps the note");
    }

    fn stash(backup_ref: &str, restored: bool) -> StashReapply {
        StashReapply {
            backup_ref: backup_ref.to_string(),
            restored,
        }
    }

    #[test]
    fn a_restored_stash_note_keeps_an_unrestored_one() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.record_merge_stash(stash("ref1", false));
        stage.record_merge_stash(stash("ref2", true));
        assert_eq!(stage.merge.stash, Some(stash("ref1", false)));
    }

    #[test]
    fn an_unrestored_stash_note_replaces_any_note() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.record_merge_stash(stash("ref1", false));
        stage.record_merge_stash(stash("ref2", false));
        assert_eq!(stage.merge.stash, Some(stash("ref2", false)));
        stage.record_merge_stash(stash("ref3", true));
        stage.merge.stash = Some(stash("ref4", true));
        stage.record_merge_stash(stash("ref5", true));
        assert_eq!(stage.merge.stash, Some(stash("ref5", true)));
    }

    #[test]
    fn leaving_merge_blocked_by_transition_clears_the_block_and_keeps_the_stash() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.record_merge_stash(stash("ref1", false));
        stage.block_merge(MergeBlock::TargetMoved);
        assert_eq!(stage.merge.block, Some(MergeBlock::TargetMoved));
        stage.try_transition(StageStatus::Queued).unwrap();
        assert_eq!(stage.merge.block, None);
        assert!(stage.failure_info.is_none());
        assert_eq!(stage.merge.stash, Some(stash("ref1", false)));
    }

    #[test]
    fn a_forced_status_change_clears_the_block() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.block_merge(MergeBlock::TargetMoved);
        stage.force_status_with_reason(StageStatus::NeedsHumanReview, "test");
        assert_eq!(stage.merge.block, None);
        assert!(stage.failure_info.is_none());
    }

    #[test]
    fn block_merge_from_a_status_without_the_edge_forces_it() {
        let mut stage = stage_in(StageStatus::Queued);
        stage.block_merge(MergeBlock::TargetMoved);
        assert_eq!(stage.status, StageStatus::MergeBlocked);
    }

    #[test]
    fn enter_merge_conflict_clears_the_block() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.block_merge(MergeBlock::TargetMoved);
        stage.enter_merge_conflict();
        assert_eq!(stage.status, StageStatus::MergeConflict);
        assert!(stage.merge_conflict);
        assert_eq!(stage.merge.block, None);
    }

    #[test]
    fn clear_merge_block_clears_the_failure_info_the_block_wrote() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.block_merge(MergeBlock::TargetMoved);
        stage.clear_merge_block();
        assert_eq!(stage.merge.block, None);
        assert!(stage.failure_info.is_none());
    }

    #[test]
    fn clear_merge_block_keeps_an_unrelated_failure_info() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.failure_info = Some(FailureInfo {
            failure_type: FailureType::InfrastructureError,
            detected_at: Utc::now(),
            evidence: vec!["disk full".to_string()],
        });
        stage.clear_merge_block();
        assert_eq!(stage.failure_info.expect("kept").evidence, ["disk full"]);
    }

    #[test]
    fn a_missing_completed_commit_is_recorded_and_an_existing_one_kept() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.record_completed_commit_if_missing(Some("aaa"));
        assert_eq!(stage.completed_commit.as_deref(), Some("aaa"));
        stage.record_completed_commit_if_missing(Some("bbb"));
        stage.record_completed_commit_if_missing(None);
        assert_eq!(stage.completed_commit.as_deref(), Some("aaa"));
    }

    #[test]
    fn zero_commit_reason_names_the_branch_target_and_requeue_command() {
        let reason = Stage::zero_commit_reason("s1", "main");
        assert!(reason.contains("loom/s1"), "{reason}");
        assert!(reason.contains("zero commits beyond main"), "{reason}");
        assert!(
            reason.contains("loom stage human-review s1 --approve"),
            "{reason}"
        );
    }

    #[test]
    fn route_to_review_records_the_reason_and_clears_the_block() {
        let mut stage = stage_in(StageStatus::Completed);
        stage.block_merge(MergeBlock::TargetMoved);
        stage.route_to_review("touches .claude/");
        assert_eq!(stage.status, StageStatus::NeedsHumanReview);
        assert_eq!(stage.review_reason.as_deref(), Some("touches .claude/"));
        assert_eq!(stage.merge.block, None);
    }

    #[test]
    fn merge_block_round_trips_and_is_omitted_when_absent() {
        let mut stage = stage_in(StageStatus::Completed);
        let plain = serde_yaml::to_string(&stage).unwrap();
        assert!(!plain.contains("merge_block"), "{plain}");
        let back: Stage = serde_yaml::from_str(&plain).unwrap();
        assert_eq!(back.merge.block, None);

        stage.block_merge(MergeBlock::UncommittedOverlap {
            paths: vec!["a.txt".to_string()],
        });
        let text = serde_yaml::to_string(&stage).unwrap();
        let back: Stage = serde_yaml::from_str(&text).unwrap();
        assert_eq!(back.merge.block, stage.merge.block);
    }

    #[test]
    fn merge_record_keeps_top_level_keys_and_round_trips() {
        let mut stage = stage_in(StageStatus::Completed);
        let plain = serde_yaml::to_string(&stage).unwrap();
        assert!(!plain.contains("merge_block:"), "{plain}");
        assert!(!plain.contains("merge_stash:"), "{plain}");

        stage.merge = MergeRecord {
            block: Some(MergeBlock::TargetMoved),
            stash: Some(StashReapply {
                backup_ref: "refs/loom/autostash/x".to_string(),
                restored: false,
            }),
        };
        let text = serde_yaml::to_string(&stage).unwrap();
        assert!(
            text.lines().any(|l| l.starts_with("merge_block:")),
            "{text}"
        );
        assert!(
            text.lines().any(|l| l.starts_with("merge_stash:")),
            "{text}"
        );
        let back: Stage = serde_yaml::from_str(&text).unwrap();
        assert_eq!(back.merge, stage.merge);
    }
}
