//! The typed reason a stage's merge was not advanced (`Stage::merge_block`)
//! and the transitions that set and clear it.

use chrono::Utc;

use super::types::{Stage, StageStatus};
use crate::git::MergeBlock;
use crate::models::failure::{FailureInfo, FailureType};

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
        self.merge_block = Some(block);
        if self.status != StageStatus::MergeBlocked && self.try_mark_merge_blocked().is_err() {
            self.force_status_with_reason(StageStatus::MergeBlocked, &sentence);
        }
    }

    /// Forget the typed merge block. Every transition out of a blocked merge
    /// calls this, so a stale reason never outlives the status it explained.
    pub fn clear_merge_block(&mut self) {
        self.merge_block = None;
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
        assert_eq!(stage.merge_block, Some(MergeBlock::TargetMoved));
        let info = stage.failure_info.expect("failure_info shows the sentence");
        assert_eq!(info.evidence, vec![MergeBlock::TargetMoved.to_string()]);
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
        assert_eq!(stage.merge_block, None);
    }

    #[test]
    fn merge_block_round_trips_and_is_omitted_when_absent() {
        let mut stage = stage_in(StageStatus::Completed);
        let plain = serde_yaml::to_string(&stage).unwrap();
        assert!(!plain.contains("merge_block"), "{plain}");
        let back: Stage = serde_yaml::from_str(&plain).unwrap();
        assert_eq!(back.merge_block, None);

        stage.block_merge(MergeBlock::UncommittedOverlap {
            paths: vec!["a.txt".to_string()],
        });
        let text = serde_yaml::to_string(&stage).unwrap();
        let back: Stage = serde_yaml::from_str(&text).unwrap();
        assert_eq!(back.merge_block, stage.merge_block);
    }
}
