//! Running each runner's selection and judging what it printed.

use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

use super::{ImpactOutcome, FULL_SUITE, IMPACT_TIMEOUT};
use crate::testrun::{classify, RunOutcome, RunOutput, TestRunnerAdapter, TestTarget};
use crate::verify::criteria::{ProbeRun, ProbeRunner};

/// The selections of one completion check and what they left.
pub(super) struct Selection<'a> {
    /// The checkout package directories are relative to.
    root: &'a Path,
    runner: &'a dyn ProbeRunner,
    ran: Vec<String>,
    notes: BTreeSet<String>,
    failures: Vec<String>,
}

impl<'a> Selection<'a> {
    pub(super) fn new(
        root: &'a Path,
        runner: &'a dyn ProbeRunner,
        notes: BTreeSet<String>,
    ) -> Self {
        Self {
            root,
            runner,
            ran: Vec::new(),
            notes,
            failures: Vec::new(),
        }
    }

    /// Run `adapter`'s selection of `targets` in `package` and judge it.
    pub(super) fn run_group(
        &mut self,
        package: &Path,
        adapter: &dyn TestRunnerAdapter,
        targets: &[TestTarget],
    ) -> Result<()> {
        let package_dir = self.root.join(package);
        if adapter.language() == "javascript" && missing_node_modules(&package_dir, self.root) {
            let name = package_label(package);
            let note = format!(
                "`{name}` has no node_modules in this worktree (a plan `provision` entry \
                 installs it); {FULL_SUITE}"
            );
            self.notes.insert(note);
            return Ok(());
        }
        let Some(command) = adapter.select_command(targets, &package_dir) else {
            let runner = adapter.name();
            let note = format!("{runner} cannot select tests by file; {FULL_SUITE}");
            self.notes.insert(note);
            return Ok(());
        };
        let run = self
            .runner
            .run(&command, &package_dir)
            .with_context(|| format!("failed to run `{command}`"))?;
        if run.timed_out {
            let secs = IMPACT_TIMEOUT.as_secs();
            let note = format!("`{command}` timed out after {secs} s; {FULL_SUITE}");
            self.notes.insert(note);
            return Ok(());
        }
        self.judge(command, adapter, &run, targets);
        Ok(())
    }

    /// Record a finished run as passed, a note, or a failure naming its tests.
    fn judge(
        &mut self,
        command: String,
        adapter: &dyn TestRunnerAdapter,
        run: &ProbeRun,
        targets: &[TestTarget],
    ) {
        let output = RunOutput {
            stdout: &run.stdout,
            stderr: &run.stderr,
            exit_code: run.exit_code,
        };
        if run.exit_code == Some(127) {
            let note = format!("`{command}` could not start (exit 127); {FULL_SUITE}");
            self.notes.insert(note);
            return;
        }
        let tests = selected(targets);
        let runner = adapter.name();
        match classify(&adapter.parse(&output), run.exit_code) {
            RunOutcome::Passed => self.ran.push(command),
            RunOutcome::NotSelected => {
                let note = format!("`{command}` selected no test; {FULL_SUITE}");
                self.notes.insert(note);
            }
            RunOutcome::Failed => self.failures.push(format!("`{command}` fails ({tests})")),
            RunOutcome::BuildFailed => {
                let failure = format!("`{command}` does not build ({tests})");
                self.failures.push(failure);
            }
            RunOutcome::Unparsed if run.exit_code == Some(0) => {
                let note = format!(
                    "`{command}` passed on its exit code alone ({runner} output not parsed)"
                );
                self.notes.insert(note);
                self.ran.push(command);
            }
            RunOutcome::Unparsed => {
                let exit = run
                    .exit_code
                    .map_or_else(|| "a signal".to_string(), |code| format!("exit {code}"));
                let failure =
                    format!("`{command}` ended with {exit} ({runner} output not parsed; {tests})");
                self.failures.push(failure);
            }
        }
    }

    /// The outcome, or an error listing every failed selection.
    pub(super) fn finish(self, stage_id: &str) -> Result<ImpactOutcome> {
        if self.failures.is_empty() {
            return Ok(ImpactOutcome {
                ran: self.ran,
                notes: self.notes.into_iter().collect(),
            });
        }
        bail!(
            "impact-selected tests failed for stage '{stage_id}':\n  - {}\n\
             These tests reach code the stage changed: fix the regression, then complete the \
             stage again.",
            self.failures.join("\n  - ")
        )
    }
}

/// The package as a note names it: its checkout-relative path, `.` for the root.
fn package_label(package: &Path) -> String {
    if package.as_os_str().is_empty() {
        ".".to_string()
    } else {
        package.display().to_string()
    }
}

/// Whether the JS package in `package_dir` declares dependencies yet has no
/// `node_modules` there or in any ancestor up to and including `root`. A
/// dependency-free package needs no install, so it never counts as missing; an
/// unreadable or unparsable manifest counts as declaring dependencies.
fn missing_node_modules(package_dir: &Path, root: &Path) -> bool {
    declares_dependencies(package_dir)
        && !package_dir
            .ancestors()
            .take_while(|dir| dir.starts_with(root))
            .any(|dir| dir.join("node_modules").is_dir())
}

/// Whether `package_dir/package.json` lists a `dependencies` or
/// `devDependencies` entry.
fn declares_dependencies(package_dir: &Path) -> bool {
    let Ok(manifest) = std::fs::read_to_string(package_dir.join("package.json")) else {
        return true;
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&manifest) else {
        return true;
    };
    ["dependencies", "devDependencies"].iter().any(|key| {
        manifest
            .get(key)
            .and_then(serde_json::Value::as_object)
            .is_some_and(|entries| !entries.is_empty())
    })
}

/// The selected tests, as a failure names them.
fn selected(targets: &[TestTarget]) -> String {
    let names: Vec<&str> = targets
        .iter()
        .map(|target| target.name.as_deref().unwrap_or(&target.file))
        .collect();
    format!("selected: {}", names.join(", "))
}
