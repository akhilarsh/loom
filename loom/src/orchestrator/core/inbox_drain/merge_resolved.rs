//! `loom stage merge <own> --resolved`, relayed from a Merge session.
//!
//! The resolver merged the target into the stage branch in the stage
//! worktree. The daemon checks that worktree through pinned git
//! (`check_resolved_worktree`: no merge in progress, no unmerged path, no
//! tracked change, the current target tip contained), then lands the merge
//! with `merge_stage` through the merge gate. `merged = true` is written only
//! after ancestry proves the stage's commit is in the target. The worktree is
//! not removed here: the resolver is still running in it, and its exit
//! (`handle_merge_session_completed`) cleans up.

use crate::git::merge::check_resolved_worktree;
use crate::models::session::Session;
use crate::models::stage::StageStatus;

use super::super::merge_handler::Landing;
use super::super::persistence::Persistence;
use super::super::Orchestrator;
use super::Settle;

impl Orchestrator {
    /// Land the merge `session` resolved for `stage_id`.
    pub(super) fn resolve_merge_from_inbox(
        &mut self,
        _session: &Session,
        stage_id: &str,
    ) -> Settle {
        let stage = match self.load_stage(stage_id) {
            Ok(stage) => stage,
            Err(error) => return Settle::Refused(format!("{error:#}")),
        };
        if !matches!(
            stage.status,
            StageStatus::MergeConflict | StageStatus::MergeBlocked
        ) {
            return Settle::Refused(format!(
                "stage '{stage_id}' is {}, not MergeConflict or MergeBlocked",
                stage.status
            ));
        }
        let target = crate::git::branch::resolve_target_branch(
            &self.config.base_branch,
            &self.config.repo_root,
        );
        if let Err(reason) = check_resolved_worktree(&self.config.repo_root, stage_id, &target) {
            return Settle::Refused(reason);
        }
        match self.land_stage_merge(stage_id, &target) {
            Landing::Merged => Settle::Applied(Some(format!(
                "merged into '{target}'; the worktree is removed after this session exits"
            ))),
            Landing::Held => Settle::Refused(format!(
                "routed to human review: the stage branch touches a control path ({})",
                Self::CONTROL_PATHS
            )),
            Landing::Conflict(paths) => Settle::Refused(format!(
                "'{target}' moved and conflicts again in {}: merge it into this worktree \
                 again, resolve, commit, then rerun --resolved",
                paths.join(", ")
            )),
            Landing::Blocked(block) => Settle::Applied(Some(format!(
                "resolution accepted; the merge is blocked: {block}. Loom retries it every tick"
            ))),
            Landing::Unproven => Settle::Refused(format!(
                "no ancestry proof that stage '{stage_id}' landed in '{target}'; merged stays false"
            )),
            Landing::Failed(error) => Settle::Refused(error),
        }
    }
}
