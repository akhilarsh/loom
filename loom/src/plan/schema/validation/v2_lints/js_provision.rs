//! JavaScript packages need a plan `provision` entry: a stage worktree is a
//! fresh checkout with no `node_modules`, so impact-selected tests and
//! integration-verify cannot run the package's tests until something installs
//! its dependencies. Plan `version: 2` only, because `provision` is v2-only.

use std::path::{Component, Path, PathBuf};

use crate::skills::project::{PackageDetail, ProjectProfile};
use crate::testrun::registry::by_name;

use super::{LintContext, LintFinding};

/// Lockfile name and the install command that honours it, first match wins.
const INSTALLS: [(&str, &str); 5] = [
    ("bun.lock", "bun install --frozen-lockfile --ignore-scripts"),
    (
        "bun.lockb",
        "bun install --frozen-lockfile --ignore-scripts",
    ),
    (
        "pnpm-lock.yaml",
        "pnpm install --frozen-lockfile --ignore-scripts",
    ),
    (
        "yarn.lock",
        "yarn install --frozen-lockfile --ignore-scripts",
    ),
    ("package-lock.json", "npm ci --ignore-scripts"),
];

/// Install command for a package with no lockfile. Provision runs on the host in a
/// worktree a stage can edit, so every suggestion skips package lifecycle scripts.
const DEFAULT_INSTALL: &str = "npm install --ignore-scripts";

const TRUNCATED_NOTE: &str = "the package scan stopped at its depth or entry limit, so a JS \
                              package may be missing from the provision check";

/// Push the findings and return the notes.
pub(super) fn check(ctx: &LintContext<'_>, out: &mut Vec<LintFinding>) -> Vec<String> {
    let Some(root) = ctx.repo_root.filter(|_| ctx.metadata.loom.version == 2) else {
        return Vec::new();
    };
    let provision = &ctx.metadata.loom.provision;
    for (index, entry) in provision.iter().enumerate() {
        if !root.join(&entry.working_dir).is_dir() {
            out.push(plan_finding(format!(
                "provision entry #{} working_dir `{}` does not exist in the repository",
                index + 1,
                entry.working_dir
            )));
        }
    }
    let profile = ProjectProfile::discover(root);
    let covers = |package: &Path| {
        provision.iter().any(|entry| {
            let dir = normalized(Path::new(&entry.working_dir));
            normalized(package).starts_with(dir)
        })
    };
    for package in profile.package_details() {
        if let Some(runner) = js_runner(&package) {
            if !covers(&package.path) {
                let install = install_command(&profile.root.join(&package.path));
                out.push(plan_finding(uncovered_message(&package, runner, install)));
            }
        }
    }
    if profile.truncated {
        vec![TRUNCATED_NOTE.to_string()]
    } else {
        Vec::new()
    }
}

fn plan_finding(message: String) -> LintFinding {
    LintFinding {
        stage_id: None,
        message,
        error_in_v2: true,
    }
}

/// The package's test-runner adapter name when it runs JavaScript tests.
fn js_runner(package: &PackageDetail) -> Option<&'static str> {
    let runner = package.runner?;
    let is_js = by_name(runner).is_some_and(|adapter| adapter.language() == "javascript");
    is_js.then_some(runner)
}

fn uncovered_message(package: &PackageDetail, runner: &str, install: &str) -> String {
    let path = package.path.display();
    format!(
        "package `{path}` runs its tests with {runner} and no `provision` entry covers it: \
         impact-selected tests and integration-verify cannot run it in a worktree, which has \
         no node_modules; add `{{ working_dir: \"{path}\", command: \"{install}\" }}` to \
         `loom.provision`"
    )
}

/// The install command that matches the lockfile in `package_dir`.
fn install_command(package_dir: &Path) -> &'static str {
    INSTALLS
        .iter()
        .find(|(lockfile, _)| package_dir.join(lockfile).exists())
        .map_or(DEFAULT_INSTALL, |(_, command)| command)
}

/// `path` without its `.` components, so `.` and `""` both name the root and
/// `./web` equals `web`.
fn normalized(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}
