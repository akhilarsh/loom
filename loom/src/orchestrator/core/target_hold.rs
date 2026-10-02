//! The daemon's side of the target guard ([`crate::git::target_guard`]): it
//! runs the guard at startup and every tick, remembers whether the target is
//! held, and keeps held work away from the target: merges wait, merge
//! resolvers do not spawn, knowledge stages (which commit to the target's
//! checkout) do not start, and merged-ness is judged against the tip loom
//! accepted, never against a target an agent moved.

use crate::git::branch::resolve_target_branch;
use crate::git::hooks::{install_reference_transaction_hook, HookInstall};
use crate::git::target_guard::{self, AttestationMode, GuardState, Hold};
use crate::models::stage::{Stage, StageType};
use crate::orchestrator::scheduling_report::BlockReason;

use super::{clear_status_line, Orchestrator};

/// Print `line` for the operator and log it as a warning.
fn warn_operator(line: &str) {
    clear_status_line();
    eprintln!("{line}");
    tracing::warn!("{line}");
}

impl Orchestrator {
    /// The configured target branch.
    fn guard_target(&self) -> String {
        resolve_target_branch(&self.config.base_branch, &self.config.repo_root)
    }

    /// Run the target guard for the configured target and remember the
    /// result. A contended merge lock or an error keeps the last state; a
    /// hold of a newly observed tip is printed once, and an error is logged
    /// once per distinct message until the guard answers again.
    pub(in crate::orchestrator::core) fn check_target_guard(&mut self) -> Option<Hold> {
        let target = self.guard_target();
        let checked = target_guard::check(&self.config.repo_root, &self.config.work_dir, &target);
        match checked {
            Ok(Some(GuardState::Clear { .. })) => {
                self.target_hold = None;
                self.target_guard_error = None;
            }
            Ok(Some(GuardState::Held(hold))) => {
                self.target_guard_error = None;
                self.remember_hold(&target, hold);
            }
            Ok(None) => {}
            Err(error) => self.log_guard_error(&error),
        }
        self.target_hold.clone()
    }

    /// Keep `hold`, printing it when its observed tip differs from the
    /// previous hold's, or there was none.
    fn remember_hold(&mut self, target: &str, hold: Hold) {
        let new_tip = self
            .target_hold
            .as_ref()
            .is_none_or(|previous| previous.observed != hold.observed);
        if new_tip {
            warn_operator(&target_guard::hold_alert(target, &hold));
        }
        self.target_hold = Some(hold);
    }

    fn log_guard_error(&mut self, error: &anyhow::Error) {
        let message = format!("target guard: {error:#}");
        if self.target_guard_error.as_deref() != Some(message.as_str()) {
            warn_operator(&message);
            self.target_guard_error = Some(message);
        }
    }

    /// Whether the last check left the target held.
    pub(in crate::orchestrator::core) fn target_held(&self) -> bool {
        self.target_hold.is_some()
    }

    /// Ancestry of `commit` against the accepted target tip (the live
    /// `target` while the guard has no entry for it).
    pub(in crate::orchestrator::core) fn merged_into_accepted(
        &self,
        commit: &str,
        target: &str,
    ) -> anyhow::Result<bool> {
        let (repo_root, work_dir) = (&self.config.repo_root, &self.config.work_dir);
        target_guard::merged_into_accepted(repo_root, work_dir, target, commit)
    }

    /// Whether `commit` is in the accepted target tip, for a caller that
    /// reverts `merged` when it is not; no commit counts as not merged.
    /// `None` when the guard cannot be evaluated: that is not evidence of a
    /// phantom merge, and the guard keeps merges held.
    pub(in crate::orchestrator::core) fn probe_merged(
        &self,
        stage_id: &str,
        commit: Option<&str>,
        target: &str,
    ) -> Option<bool> {
        let Some(commit) = commit else {
            return Some(false);
        };
        match self.merged_into_accepted(commit, target) {
            Ok(merged) => Some(merged),
            Err(error) => {
                tracing::debug!(
                    %stage_id,
                    %error,
                    "Cannot evaluate the target guard; leaving merged unchanged"
                );
                None
            }
        }
    }

    /// Why `stage` must not spawn now: `Held` for a held stage, `TargetHeld`
    /// for a knowledge stage while the target is held.
    pub(in crate::orchestrator::core) fn spawn_hold_reason(
        &self,
        stage: &Stage,
    ) -> Option<BlockReason> {
        if stage.held {
            Some(BlockReason::Held)
        } else if stage.stage_type == StageType::Knowledge && self.target_held() {
            Some(BlockReason::TargetHeld)
        } else {
            None
        }
    }

    /// Install the reference-transaction hook and run the first check, then
    /// say what the guard holds when attestation is off.
    pub(in crate::orchestrator::core) fn start_target_guard(&mut self) {
        match install_reference_transaction_hook(&self.config.repo_root) {
            Ok(HookInstall::Installed) => {
                tracing::info!("target guard: reference-transaction hook installed");
            }
            Ok(HookInstall::UpToDate) => {
                tracing::info!("target guard: reference-transaction hook up to date");
            }
            Ok(HookInstall::ForeignHookPresent) => warn_operator(
                "target guard: .git/hooks/reference-transaction belongs to another tool and \
                 was left in place",
            ),
            Err(error) => warn_operator(&format!(
                "target guard: could not install the reference-transaction hook: {error:#}"
            )),
        }
        self.check_target_guard();
        self.report_attestation_off();
    }

    /// Print one line when attestation is off, saying what is held without it.
    fn report_attestation_off(&self) {
        let (repo_root, work_dir) = (&self.config.repo_root, &self.config.work_dir);
        let AttestationMode::Off { reason } = target_guard::attestation_mode(repo_root, work_dir)
        else {
            return;
        };
        let line = match target_guard::attestation_latched(work_dir, &self.guard_target()) {
            Ok(false) => format!(
                "target guard: attestation off ({reason}); only control-path changes, \
                 rewrites of the target and unmerged stage work are held"
            ),
            Ok(true) => format!(
                "target guard: attestation off ({reason}), but this run recorded it on; \
                 every move without a ledger line holds until loom target accept"
            ),
            Err(error) => format!(
                "target guard: attestation off ({reason}); the guard record could not be \
                 read: {error:#}"
            ),
        };
        warn_operator(&line);
    }
}

#[cfg(test)]
#[path = "target_hold_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "target_hold_recovery_tests.rs"]
mod recovery_tests;
