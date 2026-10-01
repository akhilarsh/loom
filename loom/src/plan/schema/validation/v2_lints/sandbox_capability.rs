//! Commands the stage sandbox cannot serve: a network binary with no domain
//! granted, and a resource no sandbox grant reaches. Registry installs are
//! left to `registry_domains`, which names the domains they need. Also warns
//! on `allow_write` entries inside the worktree, which loom drops.

use crate::plan::schema::{NetworkConfig, StageDefinition};

use super::super::criterion_hazards::{criterion_needs_ungrantable_resource, scan, Hazard};
use super::{stage_commands, LintContext, LintFinding};

/// Install hazards `registry_domains` reports with the domains they need.
const REGISTRY_INSTALLS: [&str; 3] = ["npm install", "bun install", "cargo install"];

pub(super) fn check(ctx: &LintContext<'_>, out: &mut Vec<LintFinding>) {
    let plan_sandbox = &ctx.metadata.loom.sandbox;
    report_in_tree_grants(None, &plan_sandbox.filesystem.allow_write, out);
    for stage in &ctx.metadata.loom.stages {
        let network = stage
            .sandbox
            .network
            .as_ref()
            .unwrap_or(&plan_sandbox.network);
        check_commands(stage, network, out);
        if let Some(filesystem) = &stage.sandbox.filesystem {
            report_in_tree_grants(Some(stage), &filesystem.allow_write, out);
        }
    }
}

/// Warn on every `allow_write` entry that names a concrete path inside the
/// worktree. The session's working directory is writable already, and
/// `sandbox::build_settings` drops such an entry: emitted, it would become a
/// bind mount that git cannot unlink or rename (`Device or resource busy`).
fn report_in_tree_grants(
    stage: Option<&StageDefinition>,
    allow_write: &[String],
    out: &mut Vec<LintFinding>,
) {
    for entry in allow_write.iter().map(|entry| entry.trim()) {
        if names_in_tree_path(entry) {
            out.push(LintFinding {
                stage_id: stage.map(|stage| stage.id.clone()),
                message: format!(
                    "allow_write `{entry}` is inside the worktree, which is already writable; \
                     loom drops it"
                ),
                error_in_v2: false,
            });
        }
    }
}

/// A relative, non-glob path that climbs nowhere and does not pass through
/// the `.loom/work` or `.work` state symlink (whose target lies outside the
/// worktree). Absolute, `~/` and `$VAR` entries cannot be placed before the
/// worktree exists, so they are left alone.
fn names_in_tree_path(entry: &str) -> bool {
    !entry.is_empty()
        && !entry.starts_with(['/', '~', '$'])
        && !entry.contains(['*', '?', '[', '{'])
        && !entry.split('/').any(|part| part == "..")
        && entry != ".work"
        && !entry.starts_with(".loom/")
        && !entry.starts_with(".work/")
}

fn check_commands(stage: &StageDefinition, network: &NetworkConfig, out: &mut Vec<LintFinding>) {
    let no_domains = network.allowed_domains.is_empty() && network.additional_domains.is_empty();
    for command in stage_commands(stage) {
        if no_domains {
            for hazard in scan(command.text, false) {
                if let Hazard::Network(tool) = hazard {
                    if REGISTRY_INSTALLS.contains(&tool) {
                        continue;
                    }
                    let problem = format!(
                        "runs `{tool}` while the stage's sandbox allows no network domain; add \
                         the host it reaches to `sandbox.network.allowed_domains` or \
                         `sandbox.network.additional_domains`"
                    );
                    out.push(LintFinding::in_stage(
                        stage,
                        command.describe(&problem),
                        true,
                    ));
                }
            }
        }
        if let Some(what) = criterion_needs_ungrantable_resource(command.text) {
            let problem = format!(
                "invokes `{what}`, which needs shared .loom state or a host daemon that no \
                 sandbox grant reaches"
            );
            out.push(LintFinding::in_stage(
                stage,
                command.describe(&problem),
                true,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::schema::{FilesystemConfig, LoomConfig, LoomMetadata};

    fn owned(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| entry.to_string()).collect()
    }

    fn in_tree_findings(plan: &[&str], stage: &[&str]) -> Vec<LintFinding> {
        let mut metadata = LoomMetadata {
            loom: LoomConfig {
                version: 2,
                stages: vec![StageDefinition {
                    id: "s1".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        };
        metadata.loom.sandbox.filesystem.allow_write = owned(plan);
        metadata.loom.stages[0].sandbox.filesystem = Some(FilesystemConfig {
            allow_write: owned(stage),
            ..FilesystemConfig::default()
        });
        let ctx = LintContext {
            metadata: &metadata,
            repo_root: None,
        };
        let mut out = Vec::new();
        check(&ctx, &mut out);
        out.retain(|finding| finding.message.contains("is inside the worktree"));
        out
    }

    #[test]
    fn only_concrete_in_tree_grants_warn() {
        let findings = in_tree_findings(
            &[
                "dist",
                "src/**",
                "/abs/cache",
                "~/cache",
                "${HOME}/cache",
                "../sibling",
                ".loom/work/handoffs",
                ".work",
            ],
            &["vitest.config.ts"],
        );

        let expected = |stage_id: Option<&str>, entry: &str| LintFinding {
            stage_id: stage_id.map(str::to_string),
            message: format!(
                "allow_write `{entry}` is inside the worktree, which is already writable; \
                 loom drops it"
            ),
            error_in_v2: false,
        };
        assert_eq!(
            findings,
            vec![
                expected(None, "dist"),
                expected(Some("s1"), "vitest.config.ts")
            ]
        );
    }
}
