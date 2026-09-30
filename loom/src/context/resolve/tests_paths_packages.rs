//! Two path conventions that need more than the module path as written: a Rust
//! integration test naming its own crate by Cargo package name, and a Java glob
//! import recorded as the bare package.

use std::collections::BTreeSet;

use super::fixtures::*;
use super::paths::{import_candidates, PathIndex};

fn index_of(paths: &[&str]) -> PathIndex {
    let files = paths
        .iter()
        .map(|path| (*path, source_file(path, &[], vec![])))
        .collect();
    PathIndex::build(&graph_of(files))
}

fn resolve(index: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    import_candidates(spec, from, index, &mut BTreeSet::new())
}

#[test]
fn integration_test_imports_its_own_crate_by_package_name() {
    let index = index_of(&["src/lib.rs", "tests/add_test.rs"]);

    assert_eq!(
        resolve(&index, "demo::add", "tests/add_test.rs"),
        ["src/lib.rs"]
    );
}

#[test]
fn package_name_path_reaches_a_module_file() {
    let index = index_of(&["src/lib.rs", "src/util.rs", "benches/b.rs"]);

    assert_eq!(
        resolve(&index, "demo::util::helper", "benches/b.rs"),
        ["src/util.rs"]
    );
}

#[test]
fn package_name_path_inside_a_crate_root_stays_external() {
    let index = index_of(&["src/lib.rs", "src/main.rs", "src/x/y.rs"]);

    assert!(resolve(&index, "demo::add", "src/main.rs").is_empty());
    assert!(resolve(&index, "serde::de::*", "src/x/y.rs").is_empty());
}

#[test]
fn package_name_path_needs_the_package_directory_to_hold_the_citing_file() {
    let index = index_of(&["pkg/src/lib.rs", "other/tests/x.rs", "pkg/tests/y.rs"]);

    assert!(resolve(&index, "demo::add", "other/tests/x.rs").is_empty());
    assert_eq!(
        resolve(&index, "demo::add", "pkg/tests/y.rs"),
        ["pkg/src/lib.rs"]
    );
}

#[test]
fn a_one_segment_go_path_is_the_standard_library() {
    let index = index_of(&["pkg/errors/errors.go", "main.go"]);

    assert!(resolve(&index, "errors", "main.go").is_empty());
    assert_eq!(
        resolve(&index, "pkg/errors", "main.go"),
        ["pkg/errors/errors.go"]
    );
}

#[test]
fn java_bare_package_spec_is_a_glob_over_the_package_directory() {
    let index = index_of(&[
        "src/main/java/a/b/C.java",
        "src/main/java/a/b/D.java",
        "src/main/java/app/Main.java",
    ]);
    let from = "src/main/java/app/Main.java";

    assert_eq!(
        resolve(&index, "a.b", from),
        ["src/main/java/a/b/C.java", "src/main/java/a/b/D.java"]
    );
    assert_eq!(resolve(&index, "a.b.C", from), ["src/main/java/a/b/C.java"]);
    assert_eq!(
        resolve(&index, "a.b.*", from),
        ["src/main/java/a/b/C.java", "src/main/java/a/b/D.java"]
    );
}
