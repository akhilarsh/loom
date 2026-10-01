//! The repository's own `pre-commit` hook: every stage commits, so a hook that
//! fetches from a package registry runs inside each stage's sandbox.

use std::path::{Path, PathBuf};

use crate::fs::safe_read::read_to_string_bounded;
use crate::git::{configured_hooks_path, run_git_checked};

use super::registry_domains::{allowed_patterns, missing_domains, registry_need, Domains};
use super::{visit_argvs, LintContext, LintFinding};

/// Largest hook script the lint reads.
const MAX_HOOK_BYTES: usize = 1 << 20;

pub(super) fn check(ctx: &LintContext<'_>, out: &mut Vec<LintFinding>) {
    let Some(root) = ctx.repo_root else {
        return;
    };
    let Some(hooks_dir) = hooks_dir(root) else {
        return;
    };
    let hook_name = Path::new("pre-commit");
    let Ok(script) = read_to_string_bounded(&hooks_dir, hook_name, MAX_HOOK_BYTES) else {
        return;
    };
    let mut needs: Vec<(String, Domains)> = Vec::new();
    visit_argvs(&script, 0, &mut |argv| {
        if let Some(need) = registry_need(argv) {
            if !needs.iter().any(|(tool, _)| *tool == need.0) {
                needs.push(need);
            }
        }
    });
    let hook = hooks_dir.join(hook_name);
    let shown = hook.strip_prefix(root).unwrap_or(&hook).display();
    for stage in &ctx.metadata.loom.stages {
        let Some(allowed) = allowed_patterns(ctx, stage) else {
            continue;
        };
        for (tool, needed) in &needs {
            let missing = missing_domains(&allowed, needed);
            if missing.is_empty() {
                continue;
            }
            let message = format!(
                "the repository's pre-commit hook `{shown}` runs `{tool}`, which needs {}; the \
                 stage's sandbox does not allow {}, so the hook cannot do its work when the \
                 stage commits: allow the domain or change the hook",
                needed.join(", "),
                missing.join(", ")
            );
            out.push(LintFinding::in_stage(stage, message, true));
        }
    }
}

/// The directory git runs hooks from: the configured `core.hooksPath` or the common
/// git directory's `hooks`. `None` when git cannot say.
fn hooks_dir(root: &Path) -> Option<PathBuf> {
    let common = run_git_checked(&["rev-parse", "--git-common-dir"], root).ok()?;
    Some(resolve_hooks_dir(
        root,
        configured_hooks_path(root).as_deref(),
        &common,
    ))
}

/// `configured` when set (a relative value resolves against `root`), else the
/// `hooks` directory of `common_dir`, which git reports relative to `root` or absolute.
fn resolve_hooks_dir(root: &Path, configured: Option<&str>, common_dir: &str) -> PathBuf {
    match configured {
        Some(dir) => root.join(dir),
        None => root.join(common_dir.trim()).join("hooks"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_hooks_path_is_the_common_git_dir_hooks() {
        let root = Path::new("/repo");
        assert_eq!(
            resolve_hooks_dir(root, None, ".git\n"),
            Path::new("/repo/.git/hooks")
        );
        assert_eq!(
            resolve_hooks_dir(root, None, "/main/.git"),
            Path::new("/main/.git/hooks")
        );
    }

    #[test]
    fn a_configured_hooks_path_wins_and_resolves_against_the_root() {
        let root = Path::new("/repo");
        assert_eq!(
            resolve_hooks_dir(root, Some("hooks"), ".git"),
            Path::new("/repo/hooks")
        );
        assert_eq!(
            resolve_hooks_dir(root, Some("/abs/hooks"), ".git"),
            Path::new("/abs/hooks")
        );
    }
}
