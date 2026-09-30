//! What a qualifier or a receiver an import binds may reach: an import decides
//! a qualified call first, leading segments dropped from a spelling must name
//! something here, and an imported item is never its module.

use super::fixtures::*;
use super::tests_rules::{assert_bound, assert_candidates, caller, defining, first_edge};
use crate::context::source_graph::EdgeProvenance::Import;
use crate::context::source_graph::{SourceEdge, SourceNodeKind as K, UNRESOLVED_TARGET};

const RUN: Symbols<'static> = &[(K::Function, &["run"])];

/// A local `impl Error { fn new() }`, a namesake of `std::io::Error::new`.
const LOCAL_ERROR: Symbols<'static> = &[
    (K::Implementation, &["Error"]),
    (K::Function, &["Error", "new"]),
];

fn assert_gap(edge: &SourceEdge) {
    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}

#[test]
fn a_path_into_a_dependency_never_binds_a_local_suffix() {
    let edge = first_edge(
        vec![
            defining("src/lib.rs", &[]),
            defining("src/error.rs", LOCAL_ERROR),
            caller("src/a.rs", RUN, "std::io::Error::new", None, vec![]),
        ],
        "src/a.rs",
    );
    assert_gap(&edge);
}

#[test]
fn an_import_of_a_dependency_decides_before_the_scope_match() {
    let import = binding("std::io::Error", Some("Error"), None);
    let edge = first_edge(
        vec![
            defining("src/lib.rs", &[]),
            defining("src/error.rs", LOCAL_ERROR),
            caller("src/a.rs", RUN, "Error::new", None, vec![import]),
        ],
        "src/a.rs",
    );
    assert_gap(&edge);
}

#[test]
fn a_crate_or_module_path_still_reaches_the_type_it_names() {
    let widget: Symbols = &[
        (K::Type, &["Widget"]),
        (K::Implementation, &["Widget"]),
        (K::Function, &["Widget", "new"]),
    ];
    let new = nested_id("src/a.rs", K::Function, &["Widget", "new"]);
    for spelling in ["crate::a::Widget::new", "a::Widget::new"] {
        let edge = first_edge(
            vec![
                defining("src/lib.rs", &[]),
                defining("src/a.rs", widget),
                caller("src/app.rs", RUN, spelling, None, vec![]),
            ],
            "src/app.rs",
        );
        assert_bound(&edge, &new, Import);
    }
}

#[test]
fn an_imported_rust_item_is_not_its_module() {
    let import = binding("crate::gadget::Widget", Some("Widget"), None);
    let edge = first_edge(
        vec![
            defining("src/lib.rs", &[]),
            source_file("src/gadget.rs", &["new"], vec![]),
            caller("src/app.rs", RUN, "Widget::new", None, vec![import]),
        ],
        "src/app.rs",
    );
    assert_gap(&edge);
}

/// `impl From<Name> for String { fn from(..) }`: the only local trace of
/// `String` is an impl block, which does not define the type.
const FOREIGN_IMPL: Symbols<'static> = &[
    (K::Type, &["Name"]),
    (K::Implementation, &["String"]),
    (K::Function, &["String", "from"]),
];

#[test]
fn an_owner_only_impl_blocks_carry_lists_its_member_without_binding_it() {
    let from = nested_id("src/name.rs", K::Function, &["String", "from"]);
    let elsewhere = first_edge(
        vec![
            defining("src/lib.rs", &[]),
            defining("src/name.rs", FOREIGN_IMPL),
            caller("src/main.rs", RUN, "String::from", None, vec![]),
        ],
        "src/main.rs",
    );
    assert_candidates(&elsewhere, std::slice::from_ref(&from));

    let mut beside: Vec<(K, &[&str])> = FOREIGN_IMPL.to_vec();
    beside.push((K::Function, &["greeting"]));
    let same_file = first_edge(
        vec![
            defining("src/lib.rs", &[]),
            caller("src/name.rs", &beside, "String::from", None, vec![]),
        ],
        "src/name.rs",
    );
    assert_candidates(&same_file, &[from]);
}

#[test]
fn an_owner_a_type_defines_binds_and_is_recorded() {
    let files = || {
        vec![
            defining("src/lib.rs", &[]),
            defining("src/name.rs", FOREIGN_IMPL),
            defining("src/string.rs", &[(K::Type, &["String"])]),
            caller("src/main.rs", RUN, "String::from", None, vec![]),
        ]
    };
    let from = nested_id("src/name.rs", K::Function, &["String", "from"]);
    assert_bound(&first_edge(files(), "src/main.rs"), &from, Import);

    let (_, keys) = super::resolve_graph_recording(&mut graph_of_files(files()));
    let edge = super::EdgeRef {
        path: "src/main.rs".to_string(),
        index: 0,
    };
    assert!(
        keys[&edge].contains("name:rust:String"),
        "a type definition appearing or leaving must re-resolve the edge: {:?}",
        keys[&edge]
    );
}

#[test]
fn a_member_of_an_imported_value_never_binds_a_top_level_namesake() {
    let log: Symbols = &[(K::Constant, &["Logger"]), (K::Function, &["info"])];
    let import = binding("./log", Some("Logger"), Some("Logger"));
    let edge = first_edge(
        vec![
            defining("src/log.ts", log),
            caller("src/app.ts", RUN, "info", Some("Logger"), vec![import]),
        ],
        "src/app.ts",
    );
    assert_gap(&edge);
}
