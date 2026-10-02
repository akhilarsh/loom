//! The branch `loom init` records as the plan's merge target.

use crate::git::branch::current_branch;
use anyhow::{bail, Context, Result};
use std::path::Path;

/// The branch checked out in `repo_root`. Refuses a detached HEAD: stages
/// merge into a named branch, and `HEAD` is not one.
pub(super) fn checked_out_branch(repo_root: &Path) -> Result<String> {
    let branch = current_branch(repo_root).context("Failed to get current git branch")?;
    if branch == "HEAD" {
        bail!("loom init needs a checked-out branch to merge stages into; HEAD is detached");
    }
    Ok(branch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::runner::run_git_checked;

    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        run_git_checked(&["init", "-q", "-b", "main"], root).unwrap();
        run_git_checked(
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
            root,
        )
        .unwrap();
        tmp
    }

    #[test]
    fn records_the_checked_out_branch() {
        let tmp = repo();
        assert_eq!(checked_out_branch(tmp.path()).unwrap(), "main");
    }

    #[test]
    fn refuses_a_detached_head() {
        let tmp = repo();
        run_git_checked(&["checkout", "-q", "--detach"], tmp.path()).unwrap();
        let error = checked_out_branch(tmp.path()).unwrap_err();
        assert!(error.to_string().contains("HEAD is detached"));
    }
}
