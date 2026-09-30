//! What Go imports bind, and what a call on a receiver claims.

use std::path::Path;

use super::*;
use crate::context::source_graph::{EdgeProvenance, SourceEdgeKind};

fn extraction(source: &str) -> FileExtraction {
    GoExtractor::new()
        .extract(Path::new("src/lib.go"), source.as_bytes())
        .unwrap()
}

/// `(path, alias, glob, local name)` per binding, in the order the extraction
/// reports them.
fn bindings(source: &str) -> Vec<(String, Option<String>, bool, Option<String>)> {
    extraction(source)
        .imports
        .into_iter()
        .map(|binding| {
            let local = binding.local_name().map(str::to_string);
            (binding.path, binding.alias, binding.glob, local)
        })
        .collect()
}

#[test]
fn an_import_binds_its_last_segment_unless_renamed() {
    assert_eq!(
        bindings("package p\n\nimport (\n\t\"a/b\"\n\tx \"c/d\"\n)\n"),
        vec![
            ("a/b".to_string(), None, false, Some("b".to_string())),
            (
                "c/d".to_string(),
                Some("x".to_string()),
                false,
                Some("x".to_string())
            ),
        ]
    );
}

#[test]
fn a_dot_import_is_a_glob_and_a_blank_import_binds_nothing() {
    assert_eq!(
        bindings("package p\n\nimport (\n\t. \"a/b\"\n\t_ \"c/d\"\n)\n"),
        vec![
            ("a/b".to_string(), None, true, None),
            ("c/d".to_string(), Some(String::new()), false, None),
        ]
    );
}

/// Go receivers are named per method, so no receiver spelling is `self`: a
/// member call is never bound by its member name at extraction.
#[test]
fn a_call_on_a_receiver_is_not_bound_by_its_member_name() {
    let extraction = extraction(
        "package p\n\nfunc (w W) A() {\n\tw.B()\n}\n\nfunc (w W) B() {}\n\ntype W struct{}\n",
    );
    let edge = extraction
        .edges
        .iter()
        .find(|edge| edge.symbol == "B" && edge.kind == SourceEdgeKind::Calls)
        .unwrap();

    assert_eq!(edge.provenance, EdgeProvenance::Syntax);
    assert!(edge.is_unresolved());
    assert_eq!(edge.receiver.as_deref(), Some("w"));
}
