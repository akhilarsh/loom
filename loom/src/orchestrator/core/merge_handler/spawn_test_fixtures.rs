//! Repositories and orchestrators the merge-resolver spawn-loop tests share.

use std::path::Path;

use tempfile::TempDir;

use crate::fs::work_dir::write_terminal_config;
use crate::models::session::{SessionBackendKind, TerminalConfig};
use crate::models::stage::{Stage, StageStatus};
use crate::orchestrator::core::{Orchestrator, OrchestratorConfig};
use crate::plan::ExecutionGraph;
use crate::verify::transitions::save_stage;

/// Run `git` in `root` with ambient global/system config neutralized, and
/// assert it succeeded (mirrors `merge_gate_tests::git_ok`).
pub(crate) fn git_ok(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", root.join(".loom-test-no-global"))
        .env("GIT_CONFIG_SYSTEM", root.join(".loom-test-no-system"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "git {args:?} failed: {stderr}");
}

/// A repo with `main` at a seed commit and a `loom/<id>` branch for each of
/// `stage_ids`, all at that commit.
pub(crate) fn repo_with_stage_branches(stage_ids: &[&str]) -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git_ok(root, &["init", "-b", "main"]);
    git_ok(root, &["config", "user.email", "t@t.com"]);
    git_ok(root, &["config", "user.name", "t"]);
    std::fs::write(root.join("seed.txt"), "seed").unwrap();
    git_ok(root, &["add", "seed.txt"]);
    git_ok(root, &["commit", "-m", "seed"]);
    for stage_id in stage_ids {
        git_ok(root, &["branch", &format!("loom/{stage_id}")]);
    }
    temp
}

/// Commit `path` on `loom/<stage_id>`, leaving `main` checked out.
pub(crate) fn commit_on_stage_branch(root: &Path, stage_id: &str, path: &str) {
    git_ok(root, &["checkout", "-q", &format!("loom/{stage_id}")]);
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "stage work").unwrap();
    git_ok(root, &["add", path]);
    git_ok(root, &["commit", "-q", "-m", "stage work"]);
    git_ok(root, &["checkout", "-q", "main"]);
}

/// Run `pass` with the stage directory of `orchestrator` read-only, so no
/// stage file can be written; `None`, without running it, when this process
/// can write there anyway (as root can).
pub(crate) fn with_read_only_stages<R>(
    orchestrator: &mut Orchestrator,
    pass: impl FnOnce(&mut Orchestrator) -> R,
) -> Option<R> {
    let stages = orchestrator.config.work_dir.join("stages");
    let writable = std::fs::metadata(&stages).unwrap().permissions();
    let mut read_only = writable.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&stages, read_only).unwrap();
    let probe = stages.join(".write-probe");
    let result = if std::fs::write(&probe, "").is_ok() {
        std::fs::remove_file(&probe).unwrap();
        None
    } else {
        Some(pass(orchestrator))
    };
    std::fs::set_permissions(&stages, writable).unwrap();
    result
}

/// An orchestrator over `repo_root` whose work dir holds one `MergeConflict`
/// stage `stage_id`. The tmux backend keeps `Orchestrator::new` from probing
/// the host for a terminal emulator.
pub(crate) fn orchestrator_with_conflict(repo_root: &Path, stage_id: &str) -> Orchestrator {
    let work_dir = repo_root.join(".loom").join("work");
    let backend = SessionBackendKind::Tmux;
    write_terminal_config(&work_dir, &TerminalConfig { backend }).unwrap();
    let stage = Stage {
        id: stage_id.to_string(),
        status: StageStatus::MergeConflict,
        ..Stage::default()
    };
    save_stage(&stage, &work_dir).unwrap();
    let config = OrchestratorConfig {
        work_dir,
        repo_root: repo_root.to_path_buf(),
        base_branch: Some("main".to_string()),
        enable_skill_routing: false,
        ..Default::default()
    };
    Orchestrator::new(config, ExecutionGraph::build(Vec::new()).unwrap()).unwrap()
}
