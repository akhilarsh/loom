//! What Python imports bind, and what a `self` call claims.

use std::path::Path;

use super::*;
use crate::context::source_graph::{EdgeProvenance, RECEIVER_CONFIDENCE};

/// One binding as `(path, name, alias, glob)`.
type Leaf = (String, Option<String>, Option<String>, bool);

fn extraction(source: &str) -> FileExtraction {
    PythonExtractor::new()
        .extract(Path::new("src/lib.py"), source.as_bytes())
        .unwrap()
}

/// The bindings of `source`, in the order the extraction reports them: by
/// site, then path and name.
fn bindings(source: &str) -> Vec<Leaf> {
    extraction(source)
        .imports
        .into_iter()
        .map(|binding| (binding.path, binding.name, binding.alias, binding.glob))
        .collect()
}

fn leaf(path: &str, name: Option<&str>, alias: Option<&str>, glob: bool) -> Leaf {
    (
        path.to_string(),
        name.map(str::to_string),
        alias.map(str::to_string),
        glob,
    )
}

#[test]
fn a_dotted_import_binds_under_its_whole_dotted_name() {
    assert_eq!(
        bindings("import a.b\nimport c\n"),
        vec![
            leaf("a.b", None, Some("a.b"), false),
            leaf("c", None, None, false),
        ]
    );
}

#[test]
fn an_import_alias_is_kept_and_each_module_binds_separately() {
    assert_eq!(
        bindings("import a.b as c, d\n"),
        vec![
            leaf("a.b", None, Some("c"), false),
            leaf("d", None, None, false),
        ]
    );
}

#[test]
fn from_import_binds_each_name_with_its_alias() {
    assert_eq!(
        bindings("from x import y, z as w\n"),
        vec![
            leaf("x", Some("y"), None, false),
            leaf("x", Some("z"), Some("w"), false),
        ]
    );
}

#[test]
fn a_parenthesized_from_import_wrapped_over_lines_binds_each_name() {
    assert_eq!(
        bindings("from x import (\n    y,  # first\n    z as w,\n)\n"),
        vec![
            leaf("x", Some("y"), None, false),
            leaf("x", Some("z"), Some("w"), false),
        ]
    );
}

#[test]
fn a_star_import_is_a_glob() {
    assert_eq!(
        bindings("from x import *\n"),
        vec![leaf("x", None, None, true)]
    );
}

#[test]
fn a_relative_import_keeps_its_dots() {
    assert_eq!(
        bindings("from . import y\nfrom ..m import z\n"),
        vec![
            leaf(".", Some("y"), None, false),
            leaf("..m", Some("z"), None, false),
        ]
    );
}

#[test]
fn a_self_call_binds_to_the_enclosing_class_by_receiver() {
    let extraction = extraction(
        "class S:\n    def a(self):\n        self.b()\n\n    def b(self):\n        pass\n",
    );
    let edge = extraction
        .edges
        .iter()
        .find(|edge| edge.symbol == "b")
        .unwrap();

    assert_eq!(edge.to, "src/lib.py#function:S::b");
    assert_eq!(edge.provenance, EdgeProvenance::Receiver);
    assert_eq!(edge.confidence, RECEIVER_CONFIDENCE);
    assert_eq!(edge.receiver.as_deref(), Some("self"));
}
