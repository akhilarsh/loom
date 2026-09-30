//! The census's per-file probes and its `check-attr` call, against paths an
//! attacker-controlled checkout can rearrange.

use std::fs;
use std::time::{Duration, Instant};

use super::*;
use crate::context::census::tests::repo;
use crate::process::ProcessTimeoutError;

const GENERATED: &str = "// @generated\npub fn a() {}\n";

fn tracked(path: &str) -> Tracked {
    Tracked {
        path: path.to_string(),
        kind: EntryKind::Regular,
        non_utf8: None,
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_directory_in_the_path_is_never_followed() {
    let temp = repo(&[("real/a.rs", GENERATED)]);
    let root = temp.path();
    assert_eq!(
        size_of(root, &tracked("real/a.rs")),
        Some(GENERATED.len() as u64)
    );
    assert!(has_generated_marker(root, "real/a.rs"), "control");

    let outside = tempfile::tempdir().expect("tempdir");
    fs::write(outside.path().join("a.rs"), GENERATED).expect("write outside file");
    fs::remove_dir_all(root.join("real")).expect("remove directory");
    std::os::unix::fs::symlink(outside.path(), root.join("real")).expect("symlink");

    assert_eq!(size_of(root, &tracked("real/a.rs")), None);
    assert!(!has_generated_marker(root, "real/a.rs"));
}

#[cfg(unix)]
#[test]
fn a_fifo_in_place_of_a_file_is_sized_zero_and_never_read() {
    let temp = repo(&[("a.rs", GENERATED)]);
    let root = temp.path();
    fs::remove_file(root.join("a.rs")).expect("remove file");
    nix::unistd::mkfifo(&root.join("a.rs"), nix::sys::stat::Mode::S_IRWXU).expect("mkfifo");

    assert_eq!(size_of(root, &tracked("a.rs")), Some(0));
    assert!(!has_generated_marker(root, "a.rs"));
}

#[cfg(unix)]
#[test]
fn a_fifo_gitattributes_ends_at_the_deadline_instead_of_hanging() {
    let temp = repo(&[("a.rs", "pub fn a() {}\n")]);
    let root = temp.path();
    nix::unistd::mkfifo(&root.join(".gitattributes"), nix::sys::stat::Mode::S_IRWXU)
        .expect("mkfifo");
    let started = Instant::now();

    let error = attributes_within(root, &["a.rs"], Duration::from_millis(500))
        .err()
        .expect("a check-attr blocked on a FIFO must time out");

    assert!(
        error.downcast_ref::<ProcessTimeoutError>().is_some(),
        "{error:#}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn attributes_mark_vendored_and_generated_paths_through_the_stdin_file() {
    let temp = repo(&[
        (
            ".gitattributes",
            "v.rs linguist-vendored\ng.rs linguist-generated\n",
        ),
        ("v.rs", "pub fn v() {}\n"),
        ("g.rs", "pub fn g() {}\n"),
        ("p.rs", "pub fn p() {}\n"),
    ]);

    let found = attributes(temp.path(), &["v.rs", "g.rs", "p.rs"]).expect("attributes");

    assert!(found.vendored.contains("v.rs"));
    assert!(found.generated.contains("g.rs"));
    assert_eq!(found.vendored.len() + found.generated.len(), 2);
}
