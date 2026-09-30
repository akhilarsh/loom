//! What a census does with a checkout it does not trust: no code from its
//! config, no control characters from its names, no file behind a directory
//! symlink.

use std::fs;
use std::path::Path;

use super::tests::{census, counts, git, repo};
use super::CensusOptions;

/// Every character of `text` a terminal would act on, newline aside.
fn terminal_controls(text: &str) -> Vec<char> {
    text.chars()
        .filter(|ch| ch.is_control() && *ch != '\n')
        .collect()
}

#[test]
fn census_git_calls_never_run_the_repository_fsmonitor_command() {
    let temp = repo(&[("a.rs", "pub fn a() {}\n")]);
    let root = temp.path();
    let marker = root.join(".git").join("fsmonitor-ran");
    let command = format!("touch '{}'; false", marker.display());
    git(root, &["config", "core.fsmonitor", &command]);
    // The first index refresh records the fsmonitor extension; every later
    // index read runs the command unless git is told not to.
    git(root, &["status", "--porcelain"]);
    let _ = fs::remove_file(&marker);
    git(root, &["ls-files", "-s", "-z"]);
    assert!(marker.exists(), "control: the fixture command must be live");
    fs::remove_file(&marker).expect("clear the marker");

    let report = census(root);

    assert_eq!(counts(report.totals.eligible), (1, 14));
    assert!(
        !marker.exists(),
        "the census ran the repository's core.fsmonitor command"
    );
}

#[cfg(unix)]
#[test]
fn control_characters_in_tracked_names_never_reach_the_text_form() {
    let temp = repo(&[
        ("pkg\u{1b}]0;x\u{7}/Cargo.toml", "[package]\n"),
        ("pkg\u{1b}]0;x\u{7}/lib.rs", "pub fn a() {}\n"),
        ("notes.t\u{1b}]0;y\u{7}xt", "text\n"),
    ]);

    let report = census(temp.path());
    let text = report.to_string();

    assert_eq!(terminal_controls(&text), Vec::<char>::new(), "{text:?}");
    assert!(text.contains("pkg"), "{text}");
    assert!(text.contains("top unsupported extensions"), "{text}");
    let json = serde_json::to_string(&report).expect("json");
    assert_eq!(terminal_controls(&json), Vec::<char>::new(), "{json:?}");
}

#[cfg(unix)]
#[test]
fn a_file_behind_a_directory_symlink_is_dropped_not_followed() {
    let temp = repo(&[("real/a.rs", "pub fn a() {}\n")]);
    let root = temp.path();
    let outside = tempfile::tempdir().expect("tempdir");
    fs::write(outside.path().join("a.rs"), "// @generated\n").expect("write outside file");
    fs::remove_dir_all(root.join("real")).expect("remove directory");
    std::os::unix::fs::symlink(outside.path(), root.join("real")).expect("symlink");

    let totals = census(root).totals;

    let counted = totals.eligible.files
        + totals.excluded.files
        + totals.vendored.files
        + totals.generated.files
        + totals.unsupported.files;
    assert_eq!(counted, 0, "{totals:?}");
}

#[cfg(unix)]
#[test]
fn a_symlinked_root_is_censused_through_its_target() {
    let temp = repo(&[("a.rs", "pub fn a() {}\n")]);
    let links = tempfile::tempdir().expect("tempdir");
    let link = links.path().join("checkout");
    std::os::unix::fs::symlink(temp.path(), &link).expect("symlink");

    let report = census(&link);

    assert_eq!(counts(report.totals.eligible), (1, 14));
}

#[test]
fn covers_says_whether_the_current_project_is_a_census_root() {
    let project = repo(&[("a.rs", "pub fn a() {}\n")]);
    let other = repo(&[("b.rs", "pub fn b() {}\n")]);
    let options = |roots: Vec<&Path>| CensusOptions {
        roots: roots.into_iter().map(Path::to_path_buf).collect(),
    };

    assert!(options(vec![]).covers(project.path()));
    assert!(!options(vec![other.path()]).covers(project.path()));
    assert!(options(vec![other.path(), project.path()]).covers(project.path()));
    let dotted = project.path().join(".");
    assert!(options(vec![dotted.as_path()]).covers(project.path()));
}
