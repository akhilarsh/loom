//! Git version preflight for `loom run`.
//!
//! Merges are computed with `git merge-tree --write-tree --merge-base`, which
//! first shipped in git 2.40. An older git fails every merge, so `loom run`
//! refuses to start with the installed version named.

use anyhow::{bail, Result};
use std::path::Path;

use crate::git::runner::run_git_checked;

/// Oldest git that supports `git merge-tree --write-tree --merge-base`.
const MIN_GIT_VERSION: (u32, u32) = (2, 40);

/// Refuse to start when the installed git is older than [`MIN_GIT_VERSION`].
pub fn require_min_git_version(dir: &Path) -> Result<()> {
    let version_line = run_git_checked(&["--version"], dir)?;
    check_version_line(&version_line)
}

/// Parse `git version 2.53.0`, `git version 2.39.3 (Apple Git-146)` or
/// `git version 2.40.0.windows.1` into `(major, minor)`.
fn parse_git_version(line: &str) -> Option<(u32, u32)> {
    let version = line.trim().strip_prefix("git version ")?;
    let mut parts = version.split_whitespace().next()?.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn check_version_line(line: &str) -> Result<()> {
    let Some(found) = parse_git_version(line) else {
        bail!(
            "Could not read the git version from {line:?}. Loom needs \
             `git merge-tree --write-tree --merge-base` (git {}.{}+).",
            MIN_GIT_VERSION.0,
            MIN_GIT_VERSION.1
        );
    };
    if found < MIN_GIT_VERSION {
        bail!(
            "Installed git is {}.{} but loom needs `git merge-tree --write-tree \
             --merge-base` (git {}.{}+). Upgrade git and run `loom run` again.",
            found.0,
            found.1,
            MIN_GIT_VERSION.0,
            MIN_GIT_VERSION.1
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_version_strings() {
        assert_eq!(parse_git_version("git version 2.53.0"), Some((2, 53)));
        assert_eq!(
            parse_git_version("git version 2.39.3 (Apple Git-146)"),
            Some((2, 39))
        );
        assert_eq!(
            parse_git_version("git version 2.40.0.windows.1\n"),
            Some((2, 40))
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_git_version("not git"), None);
        assert_eq!(parse_git_version("git version x.y"), None);
        assert!(check_version_line("???").is_err());
    }

    #[test]
    fn minimum_is_enforced() {
        assert!(check_version_line("git version 2.40.0").is_ok());
        assert!(check_version_line("git version 3.0.1").is_ok());
        let err = check_version_line("git version 2.39.3 (Apple Git-146)").unwrap_err();
        let text = err.to_string();
        assert!(text.contains("2.39"), "{text}");
        assert!(text.contains("merge-tree"), "{text}");
    }
}
