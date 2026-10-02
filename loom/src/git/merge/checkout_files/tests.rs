use super::*;
use crate::git::merge::test_support::{commit_file, git_ok, init_repo};
use std::os::unix::fs::symlink;

#[test]
fn a_failed_removal_restores_the_files_already_removed() {
    let repo = init_repo();
    let root = repo.path();
    commit_file(root, "x.txt", "bytes", "x");
    std::fs::create_dir(root.join("d")).unwrap();

    let paths = vec!["x.txt".to_string(), "d".to_string()];
    let error = remove_files(root, "HEAD", &paths).unwrap_err();

    assert!(error.to_string().contains('d'), "{error:#}");
    assert_eq!(
        std::fs::read_to_string(root.join("x.txt")).unwrap(),
        "bytes"
    );
}

#[test]
fn restoring_keeps_the_exec_bit_of_the_blob() {
    let repo = init_repo();
    let root = repo.path();
    commit_file(root, "run.sh", "#!/bin/sh\n", "script");
    git_ok(root, &["update-index", "--chmod=+x", "run.sh"]);
    git_ok(root, &["commit", "-m", "exec"]);
    commit_file(root, "plain.txt", "plain", "plain");
    let paths = vec!["run.sh".to_string(), "plain.txt".to_string()];
    remove_files(root, "HEAD", &paths).unwrap();
    assert!(!root.join("run.sh").exists());

    restore_files(root, "HEAD", &paths);

    let mode = |name: &str| {
        std::fs::metadata(root.join(name))
            .unwrap()
            .permissions()
            .mode()
    };
    assert_eq!(mode("run.sh") & 0o111, 0o111);
    assert_eq!(mode("plain.txt") & 0o111, 0);
    assert_eq!(
        std::fs::read_to_string(root.join("run.sh")).unwrap(),
        "#!/bin/sh\n"
    );
}

#[test]
fn the_probe_reads_the_disk_and_compares_bytes() {
    let repo = init_repo();
    let root = repo.path();
    commit_file(root, "blob.txt", "same", "blob");
    std::fs::create_dir(root.join("dir")).unwrap();
    symlink("a.txt", root.join("link")).unwrap();
    let probe = DiskProbe {
        repo: root,
        base: "HEAD~1",
        tree: "HEAD",
    };

    assert_eq!(probe.disk("blob.txt"), DiskEntry::RegularFile);
    assert_eq!(probe.disk("dir"), DiskEntry::Directory);
    assert_eq!(probe.disk("link"), DiskEntry::Other);
    assert_eq!(probe.disk("missing"), DiskEntry::Absent);
    assert_eq!(probe.disk("a.txt/below"), DiskEntry::Absent);
    assert!(probe.equals_blob("blob.txt"));
    std::fs::write(root.join("blob.txt"), "different").unwrap();
    assert!(!probe.equals_blob("blob.txt"));
    assert!(!probe.equals_blob("link"), "a symlink never equals a blob");
    assert!(probe.in_base_tree("a.txt"));
    assert!(!probe.in_base_tree("blob.txt"));
}
