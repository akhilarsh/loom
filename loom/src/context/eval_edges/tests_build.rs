//! Which files of a corpus directory become graph files.

use std::fs;

use tempfile::TempDir;

use super::build::build_graph;
use super::labels::load_labels;

#[test]
fn only_files_outside_the_excluded_roots_are_extracted() {
    let dir = TempDir::new().expect("tempdir");
    let files = [
        ("labels.yaml", "dialect: rust\n"),
        ("src/lib.rs", "fn a() {}\n"),
        ("target/debug/gen.rs", "fn generated() {}\n"),
        (".loom/cache/x.rs", "fn cached() {}\n"),
        ("node_modules/dep/index.rs", "fn vendored() {}\n"),
    ];
    for (path, text) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
        fs::write(full, text).expect("write");
    }
    let labels = load_labels(dir.path()).expect("labels");

    let graph = build_graph(dir.path(), &labels).expect("graph");

    let extracted: Vec<&str> = graph.files.keys().map(String::as_str).collect();
    assert_eq!(extracted, ["src/lib.rs"]);
}
