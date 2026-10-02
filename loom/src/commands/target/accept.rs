//! `loom target accept --to <commit>`: the operator's review of a move of the
//! target that loom did not make.

use anyhow::{bail, Result};
use std::path::Path;

use super::resolve_context;
use crate::git::branch::current_branch;
use crate::git::runner::run_git;
use crate::git::target_guard::{
    abbrev, accept, attestation_mode, target_key, Accepted, AttestationMode,
};

/// Accept the target's current tip, which `to` must name.
pub fn execute(to: &str) -> Result<()> {
    refuse_inside_session()?;
    let (work_dir, repo_root, target) = resolve_context()?;
    let accepted = accept(&repo_root, &work_dir, &target, to)?;
    println!(
        "Accepted {} at {} (was {}).",
        target_key(&target),
        abbrev(&accepted.to),
        abbrev(&accepted.from)
    );
    // `accept` recorded the current mode as the run's attestation policy.
    match attestation_mode(&repo_root, &work_dir) {
        AttestationMode::Active => println!("Attestation: on"),
        AttestationMode::Off { reason } => println!("Attestation: off ({reason})"),
    }
    // The accept is recorded; a failed note must not turn it into an error exit.
    match checkout_note(&repo_root, &target, &accepted) {
        Ok(Some(note)) => println!("{note}"),
        Ok(None) => {}
        Err(error) => eprintln!("warning: could not check your checkout of the target: {error:#}"),
    }
    Ok(())
}

/// A guard rail, not the boundary: no session can write `.loom/`, which the
/// record lives in. It keeps a stage agent from trying.
pub(super) fn refuse_inside_session() -> Result<()> {
    match std::env::var("LOOM_SESSION_ID") {
        Ok(id) if !id.is_empty() => bail!(
            "loom target accept is the operator's review of a move loom did not make, and \
             this is loom session {id}; a stage agent cannot accept it"
        ),
        _ => Ok(()),
    }
}

/// The note for an operator whose checkout of the target is behind the
/// accepted tip: the checkout holds the files from before the move. `None`
/// when the target is not checked out in `repo_root` or its index is already
/// at the accepted tip. Loom never runs the command it names.
pub(crate) fn checkout_note(
    repo_root: &Path,
    target: &str,
    accepted: &Accepted,
) -> Result<Option<String>> {
    let key = target_key(target);
    if current_branch(repo_root)? != key {
        return Ok(None);
    }
    let staged = run_git(&["diff", "--cached", "--quiet", &accepted.to], repo_root)?;
    if staged.status.code() != Some(1) {
        return Ok(None);
    }
    Ok(Some(format!(
        "Your checkout of {key} still holds files from before the move. Bring it along \
         (keeps your local edits): git read-tree -m -u {} {}",
        accepted.from, accepted.to
    )))
}
