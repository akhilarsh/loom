//! The check run on a stage worktree after its merge resolver reports done:
//! the resolver merged the target into the stage branch there and committed,
//! so `merge_stage` can now land the stage without a conflict.
//!
//! Every git command runs through [`WorktreeGit::pinned`]: the agent
//! controls the worktree's `.git` file, and the daemon must not run git as
//! that file directs.

use std::path::Path;

use crate::git::branch::branch_name_for_stage;
use crate::git::runner::run_git_checked;
use crate::git::worktree::WorktreeGit;

/// Verify that the resolver left the worktree of `stage_id` ready to merge
/// into `target_branch`. `Err` is the reason, worded for the resolver and the
/// operator.
///
/// The worktree must be registered, sit on `loom/<stage_id>`, have no merge
/// in progress, no unmerged paths and no tracked change, and its HEAD must
/// contain the current tip of `target_branch`. Untracked files are allowed:
/// a sandbox leaves stub files behind.
pub fn check_resolved_worktree(
    repo_root: &Path,
    stage_id: &str,
    target_branch: &str,
) -> Result<(), String> {
    let path = repo_root.join(".worktrees").join(stage_id);
    let git = WorktreeGit::pinned(repo_root, &path).map_err(|error| {
        format!(
            "the stage worktree {} cannot be checked: {error:#}",
            path.display()
        )
    })?;
    let branch = branch_name_for_stage(stage_id);
    let (_, head_ref) = query(&git, &["symbolic-ref", "-q", "HEAD"])?;
    if head_ref != format!("refs/heads/{branch}") {
        return Err(format!(
            "the worktree is not on {branch} (HEAD is {}); switch back to it",
            if head_ref.is_empty() {
                "detached"
            } else {
                &head_ref
            }
        ));
    }
    if query(&git, &["rev-parse", "-q", "--verify", "MERGE_HEAD"])?.0 {
        return Err(
            "a merge is still in progress (MERGE_HEAD exists); commit it first".to_string(),
        );
    }
    let (_, unmerged) = query(&git, &["ls-files", "-u"])?;
    if !unmerged.is_empty() {
        return Err(format!(
            "unmerged paths remain: {}",
            unmerged_paths(&unmerged)
        ));
    }
    let (_, changes) = query(&git, &["status", "--porcelain=v1", "--untracked-files=no"])?;
    if !changes.is_empty() {
        return Err(format!(
            "tracked files have uncommitted changes; commit them: {}",
            changes.lines().collect::<Vec<_>>().join(", ")
        ));
    }
    target_tip_is_merged_in(repo_root, &git, target_branch)
}

/// Run `args` in the worktree: whether git exited 0, and its trimmed stdout.
fn query(git: &WorktreeGit, args: &[&str]) -> Result<(bool, String), String> {
    let output = git.run(args).map_err(|error| {
        format!(
            "git {} failed in the stage worktree: {error:#}",
            args.join(" ")
        )
    })?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((output.status.success(), text))
}

/// The paths of `git ls-files -u` output, once each.
fn unmerged_paths(listing: &str) -> String {
    let mut paths: Vec<&str> = Vec::new();
    for path in listing.lines().filter_map(|line| line.split('\t').nth(1)) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths.join(", ")
}

/// The current tip of `target_branch` must be an ancestor of the worktree's
/// HEAD: otherwise the target moved after the resolver merged it.
fn target_tip_is_merged_in(
    repo_root: &Path,
    git: &WorktreeGit,
    target_branch: &str,
) -> Result<(), String> {
    let spec = format!("refs/heads/{target_branch}^{{commit}}");
    let tip = run_git_checked(&["rev-parse", "--verify", &spec], repo_root)
        .map_err(|error| format!("cannot read the tip of '{target_branch}': {error:#}"))?;
    let output = git
        .run(&["merge-base", "--is-ancestor", &tip, "HEAD"])
        .map_err(|error| format!("git merge-base failed in the stage worktree: {error:#}"))?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(1) => Err(format!(
            "'{target_branch}' has commits this worktree does not contain: merge \
             {target_branch} into it again, resolve, and commit"
        )),
        _ => Err(format!(
            "git merge-base failed in the stage worktree: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::merge::test_support::{commit_file, git_ok, git_out, init_repo};
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A repository with a linked worktree for stage `s` on `loom/s`, which
    /// holds one commit of its own so merging `main` makes a merge commit.
    fn repo_with_worktree() -> (TempDir, PathBuf) {
        let repo = init_repo();
        let root = repo.path();
        let wt = root.join(".worktrees").join("s");
        git_ok(
            root,
            &["worktree", "add", "-b", "loom/s", wt.to_str().unwrap()],
        );
        commit_file(&wt, "stage.txt", "stage", "stage work");
        (repo, wt)
    }

    fn check(repo: &TempDir) -> Result<(), String> {
        check_resolved_worktree(repo.path(), "s", "main")
    }

    fn refusal(repo: &TempDir) -> String {
        check(repo).expect_err("the check must refuse")
    }

    #[test]
    fn a_merged_clean_worktree_passes_and_untracked_files_are_allowed() {
        let (repo, wt) = repo_with_worktree();
        commit_file(repo.path(), "m.txt", "main", "main work");
        git_ok(&wt, &["merge", "main", "-m", "merge main"]);
        assert_eq!(check(&repo), Ok(()));
        std::fs::write(wt.join("stub.txt"), "sandbox stub").unwrap();
        assert_eq!(check(&repo), Ok(()));
    }

    #[test]
    fn an_unregistered_worktree_is_refused() {
        let repo = init_repo();
        std::fs::create_dir_all(repo.path().join(".worktrees").join("s")).unwrap();
        assert!(refusal(&repo).contains("cannot be checked"));
    }

    #[test]
    fn a_worktree_on_another_branch_is_refused() {
        let (repo, wt) = repo_with_worktree();
        git_ok(&wt, &["checkout", "-b", "other"]);
        assert!(refusal(&repo).contains("not on loom/s"));
    }

    #[test]
    fn a_merge_in_progress_is_refused_with_its_unmerged_paths() {
        let (repo, wt) = repo_with_worktree();
        commit_file(&wt, "a.txt", "stage side", "stage edit");
        commit_file(repo.path(), "a.txt", "main side", "main edit");
        let merge = crate::git::runner::run_git(&["merge", "main"], &wt).unwrap();
        assert!(!merge.status.success(), "the merge must conflict");
        assert!(refusal(&repo).contains("MERGE_HEAD"));
        let git_dir = git_out(&wt, &["rev-parse", "--absolute-git-dir"]);
        std::fs::remove_file(Path::new(&git_dir).join("MERGE_HEAD")).unwrap();
        assert!(refusal(&repo).contains("unmerged paths remain: a.txt"));
    }

    #[test]
    fn an_uncommitted_tracked_change_is_refused() {
        let (repo, wt) = repo_with_worktree();
        std::fs::write(wt.join("a.txt"), "edited").unwrap();
        assert!(refusal(&repo).contains("uncommitted changes"));
    }

    #[test]
    fn a_target_tip_missing_from_the_worktree_is_refused() {
        let (repo, _wt) = repo_with_worktree();
        commit_file(repo.path(), "m.txt", "main", "main moved on");
        assert!(refusal(&repo).contains("has commits this worktree does not contain"));
    }
}
