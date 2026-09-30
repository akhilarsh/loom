//! What TypeScript imports and re-exports bind, and what a `this` call claims.

use std::path::Path;

use super::*;
use crate::context::source_graph::{EdgeProvenance, RECEIVER_CONFIDENCE};

/// One binding as `(path, name, alias, glob)`.
type Leaf = (String, Option<String>, Option<String>, bool);

fn extraction(source: &str) -> FileExtraction {
    TypeScriptExtractor::new()
        .extract(Path::new("src/lib.ts"), source.as_bytes())
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
fn named_imports_keep_their_aliases() {
    assert_eq!(
        bindings("import { a, b as c, type D } from \"x\";\n"),
        vec![
            leaf("x", Some("D"), Some("D"), false),
            leaf("x", Some("a"), Some("a"), false),
            leaf("x", Some("b"), Some("c"), false),
        ]
    );
}

#[test]
fn a_default_import_names_the_default_export() {
    assert_eq!(
        bindings("import d from \"x\";\nimport e, { f } from 'y';\n"),
        vec![
            leaf("x", Some("default"), Some("d"), false),
            leaf("y", Some("default"), Some("e"), false),
            leaf("y", Some("f"), Some("f"), false),
        ]
    );
}

#[test]
fn a_namespace_import_binds_the_whole_module() {
    let found = extraction("import * as ns from \"x\";\n").imports;

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, None);
    assert_eq!(found[0].local_name(), Some("ns"));
}

#[test]
fn a_side_effect_import_binds_nothing() {
    let found = extraction("import \"./setup\";\n").imports;

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "./setup");
    assert_eq!(found[0].alias.as_deref(), Some(""));
    assert_eq!(found[0].local_name(), None);
}

#[test]
fn a_re_export_binds_no_local_name_and_a_star_re_export_is_a_glob() {
    assert_eq!(
        bindings("export { a as b } from \"x\";\nexport * from \"y\";\n"),
        vec![
            leaf("x", Some("a"), Some(""), false),
            leaf("y", None, None, true),
        ]
    );
}

#[test]
fn a_this_call_binds_to_the_enclosing_class_by_receiver() {
    let extraction = extraction("class S {\n  a() {\n    this.b();\n  }\n  b() {}\n}\n");
    let edge = extraction
        .edges
        .iter()
        .find(|edge| edge.symbol == "b")
        .unwrap();

    assert_eq!(edge.to, "src/lib.ts#function:S::b");
    assert_eq!(edge.provenance, EdgeProvenance::Receiver);
    assert_eq!(edge.confidence, RECEIVER_CONFIDENCE);
    assert_eq!(edge.receiver.as_deref(), Some("this"));
}
