//! What `loom stage merge` tells the operator after a retry that did not
//! merge: what the daemon does with the stage next, and the manual options.

use std::path::Path;

use crate::daemon::DaemonServer;
use crate::fs::resolve_target_branch_from_config;
use crate::git::branch::{branch_name_for_stage, resolve_target_branch};
use crate::git::MergeBlock;
use crate::models::stage::{Stage, StageStatus};
use crate::orchestrator::core::{
    merge_resolver_attempts, Orchestrator, MAX_MERGE_RESOLVER_ATTEMPTS,
};

/// Print `daemon_next_step` for `stage`, reading the resolver counter
/// through the daemon's own reader, resolving the merge target as the daemon
/// does, and checking whether a daemon is running.
pub(super) fn print_daemon_next_step(stage: &Stage, work_dir: &Path, repo_root: &Path) {
    let attempts = merge_resolver_attempts(work_dir, &stage.id);
    let running = DaemonServer::is_running(work_dir);
    let target = resolve_target_branch_from_config(work_dir, repo_root)
        .unwrap_or_else(|_| resolve_target_branch(&None, repo_root));
    let step = daemon_next_step(
        &stage.id,
        &stage.status,
        stage.merge.block.as_ref(),
        attempts,
        running,
        repo_root,
        &target,
    );
    println!("{step}");
}

/// What the daemon does next with stage `stage_id` in `status` and carrying
/// `block`, whose counter reads `attempts` merge resolvers and whose merge target is
/// `target_branch`, as its spawn loop decides it: only a
/// `MergeConflict`/`MergeBlocked` stage gets resolvers, at most
/// `MAX_MERGE_RESOLVER_ATTEMPTS` of them, and the merge gate sends a branch
/// that touches a control path to human review instead, as does a missing
/// stage or target branch. A `MergeBlocked` stage with a typed `block` gets
/// no resolver: the daemon retries its merge every tick.
fn daemon_next_step(
    stage_id: &str,
    status: &StageStatus,
    block: Option<&MergeBlock>,
    attempts: u32,
    daemon_running: bool,
    repo_root: &Path,
    target_branch: &str,
) -> String {
    if !matches!(
        status,
        StageStatus::MergeConflict | StageStatus::MergeBlocked
    ) {
        return format!("The daemon does not retry the merge of a {status} stage.");
    }
    let daemon = if daemon_running {
        "The running daemon".to_string()
    } else {
        let root = repo_root.display();
        format!("No daemon is running. Once `loom run` from {root} starts one, it")
    };
    if let (StageStatus::MergeBlocked, Some(block)) = (status, block) {
        return format!(
            "{daemon} retries this merge every tick once the block clears ({block}); it spawns \
             no merge resolver for it."
        );
    }
    let max = MAX_MERGE_RESOLVER_ATTEMPTS;
    if attempts >= max {
        return format!(
            "{daemon} routes this stage to human review: all {max} of its merge resolvers are \
             spent."
        );
    }
    let branch = branch_name_for_stage(stage_id);
    format!(
        "{daemon} spawns merge resolver {} of {max} for this stage, unless branch {branch} \
         touches a control path ({}), or branch {branch} or target branch {target_branch} is \
         missing; then it routes the stage to human review instead.",
        attempts + 1,
        Orchestrator::CONTROL_PATHS
    )
}

/// The manual ways out once a stage's fix attempts are spent.
pub(super) fn print_fix_limit_options(stage_id: &str) {
    println!("Options:");
    println!("  - Resolve conflicts manually then:  loom stage merge {stage_id} --resolved");
    println!("  - Request human review:             loom stage human-review {stage_id}");
    println!(
        "  - Skip this stage:                  loom stage skip {stage_id} --reason \"merge too complex\""
    );
}

/// Report a merge that failed with an error rather than a conflict. `stage`
/// keeps its status and carries the fix-attempt count the caller persisted.
pub(super) fn report_merge_error(
    stage: &Stage,
    work_dir: &Path,
    repo_root: &Path,
    error: &anyhow::Error,
) {
    let stage_id = stage.id.as_str();
    println!();
    println!("Merge failed with error:");
    println!("  {error}");
    println!();
    print_daemon_next_step(stage, work_dir, repo_root);
    println!();

    let (attempts, max_attempts) = (stage.fix_attempts, stage.get_effective_max_fix_attempts());
    if stage.is_at_fix_limit() {
        println!("Fix attempt limit reached ({attempts}/{max_attempts}).");
        println!();
        println!("Options:");
        println!("  - Request human review:  loom stage human-review {stage_id}");
        println!("  - Skip this stage:       loom stage skip {stage_id} --reason \"merge error\"");
    } else {
        println!(
            "Remaining attempts: {}/{max_attempts}",
            max_attempts - attempts
        );
        println!("Investigate the error and try again: loom stage merge {stage_id}");
    }
}

#[cfg(test)]
mod tests {
    use super::daemon_next_step;
    use crate::git::MergeBlock;
    use crate::models::stage::StageStatus;
    use std::path::Path;

    fn next_step(status: StageStatus, attempts: u32, daemon_running: bool) -> String {
        let repo = Path::new("/repo");
        daemon_next_step("s", &status, None, attempts, daemon_running, repo, "trunk")
    }

    #[test]
    fn budget_left_promises_a_resolver_unless_the_gate_holds_the_branch() {
        let running = next_step(StageStatus::MergeConflict, 0, true);
        assert!(running.starts_with("The running daemon spawns merge resolver 1 of 6"));
        assert!(running.contains("unless branch loom/s touches a control path"));
        assert!(running.contains(".mcp.json") && running.contains("human review"));
        assert!(running.contains("or branch loom/s or target branch trunk is missing"));

        let stopped = next_step(StageStatus::MergeBlocked, 2, false);
        assert!(stopped.starts_with("No daemon is running. Once `loom run` from /repo"));
        assert!(stopped.contains("spawns merge resolver 3 of 6"));
    }

    #[test]
    fn a_spent_budget_promises_human_review_and_no_resolver() {
        for running in [true, false] {
            let step = next_step(StageStatus::MergeConflict, 6, running);
            assert!(step.contains("routes this stage to human review"), "{step}");
            assert!(!step.contains("spawns"), "{step}");
        }
        let stopped = next_step(StageStatus::MergeBlocked, 6, false);
        assert!(stopped.starts_with("No daemon is running"));
    }

    #[test]
    fn a_typed_block_promises_a_retry_every_tick_and_no_resolver() {
        let repo = Path::new("/repo");
        let block = MergeBlock::TargetMoved;
        for running in [true, false] {
            let step = daemon_next_step(
                "s",
                &StageStatus::MergeBlocked,
                Some(&block),
                0,
                running,
                repo,
                "trunk",
            );
            assert!(
                step.contains("retries this merge every tick once the block clears"),
                "{step}"
            );
            assert!(step.contains(&block.to_string()), "{step}");
            assert!(step.contains("spawns no merge resolver for it"), "{step}");
        }
    }

    #[test]
    fn a_block_on_a_conflict_stage_is_ignored() {
        let block = MergeBlock::TargetMoved;
        let step = daemon_next_step(
            "s",
            &StageStatus::MergeConflict,
            Some(&block),
            0,
            true,
            Path::new("/repo"),
            "trunk",
        );
        assert!(step.contains("spawns merge resolver 1 of 6"), "{step}");
    }

    #[test]
    fn a_completed_stage_gets_no_daemon_retry() {
        let step = next_step(StageStatus::Completed, 0, true);
        assert_eq!(
            step,
            "The daemon does not retry the merge of a Completed stage."
        );
    }
}
