//! Re-exports a lookup follows and where it stops: a renamed ES re-export, a
//! re-export cycle, the prototype exemption outside the C family, and an alias
//! whose parent module lacks the item it names.

use super::fixtures::*;
use super::tests_rules::{assert_bound, assert_candidates, caller, defining, first_edge};
use crate::context::graph_store::FileEntry;
use crate::context::source_graph::EdgeProvenance::Import;
use crate::context::source_graph::SourceEdgeKind::References;
use crate::context::source_graph::UNRESOLVED_TARGET;
use crate::context::source_graph::{ImportBinding, SourceEdge, SourceNodeKind as K};

const RUN: Symbols<'static> = &[(K::Function, &["run"])];

/// `export { name as exported } from "<path>"`: binds nothing in its file and
/// makes the module export `name` as `exported`.
fn reexport(path: &str, name: &str, exported: &str) -> ImportBinding {
    ImportBinding {
        exported_as: Some(exported.to_string()),
        ..binding(path, Some(name), Some(""))
    }
}

/// A file defining nothing, holding only `imports`.
fn importing(path: &str, imports: Vec<ImportBinding>) -> FileEntry {
    dialect_file(path, &[], vec![], imports)
}

fn assert_gap(edge: &SourceEdge) {
    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}

#[test]
fn a_renamed_reexport_binds_the_definition_it_renames() {
    let import = binding("./index", Some("biggest"), Some("biggest"));
    let renamed = reexport("./shapes", "largest", "biggest");
    let edge = first_edge(
        vec![
            importing("src/index.js", vec![renamed]),
            source_file("src/shapes.js", &["largest"], vec![]),
            source_file("src/other.js", &["largest"], vec![]),
            caller("src/app.js", RUN, "biggest", None, vec![import]),
        ],
        "src/app.js",
    );
    assert_bound(&edge, &func_id("src/shapes.js", "largest"), Import);
}

#[test]
fn a_reexport_cycle_ends_at_the_hop_limit_unbound() {
    let edge = first_edge(
        vec![
            importing("shop/__init__.py", vec![binding(".a", Some("f"), None)]),
            importing("shop/a.py", vec![binding(".", Some("f"), None)]),
            caller(
                "main.py",
                RUN,
                "f",
                None,
                vec![binding("shop", Some("f"), None)],
            ),
        ],
        "main.py",
    );
    assert_gap(&edge);
}

#[test]
fn a_same_named_reference_lifts_a_refusal_only_in_the_c_family() {
    let external = vec![glob_binding("external_crate")];
    let mut app = caller("src/app.rs", RUN, "helper", None, external);
    let run = func_id("src/app.rs", "run");
    app.edges.push(unresolved_edge(run, References, "helper"));
    let edge = first_edge(
        vec![app, source_file("src/b.rs", &["helper"], vec![])],
        "src/app.rs",
    );
    assert_candidates(&edge, &[func_id("src/b.rs", "helper")]);
}

#[test]
fn an_alias_whose_parent_module_lacks_the_item_binds_nothing() {
    let core: Symbols = &[(K::Module, &["App.Core"]), (K::Function, &["W", "Run"])];
    let util: Symbols = &[
        (K::Module, &["App.Util"]),
        (K::Function, &["Text", "Shout"]),
    ];
    let elsewhere: Symbols = &[
        (K::Module, &["App.Other"]),
        (K::Type, &["Strings"]),
        (K::Function, &["Strings", "Shout"]),
    ];
    let alias = binding("App.Util.Strings", Some("Strings"), Some("S"));
    let edge = first_edge(
        vec![
            caller("src/Core/W.cs", core, "Shout", Some("S"), vec![alias]),
            defining("src/Util/Text.cs", util),
            defining("src/Other/Strings.cs", elsewhere),
        ],
        "src/Core/W.cs",
    );
    assert_gap(&edge);
}
