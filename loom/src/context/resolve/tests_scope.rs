//! Names another scope brings into view: a re-export, a type alias, a type a
//! glob import makes visible, a prototype, a constructor and a used trait.
//! One test per shape, graphs built by hand as the extractors emit them.

use super::fixtures::*;
use super::tests_rules::{assert_bound, assert_candidates, caller, defining, first_edge};
use super::*;
use crate::context::source_graph::EdgeProvenance::{Import, Receiver, UniqueName};
use crate::context::source_graph::SourceEdgeKind::{Calls, References};
use crate::context::source_graph::{SourceNodeKind as K, UNRESOLVED_TARGET};

const RUN: Symbols<'static> = &[(K::Function, &["run"])];

#[test]
fn a_renamed_python_reexport_binds_the_definition_it_names() {
    let import = binding("shop", Some("compute_total"), None);
    let reexport = binding(".pricing", Some("total"), Some("compute_total"));
    let package = dialect_file("shop/__init__.py", &[], vec![], vec![reexport]);
    let mut graph = graph_of_files(vec![
        package,
        source_file("shop/pricing.py", &["total"], vec![]),
        source_file("shop/other.py", &["total"], vec![]),
        caller("shop/cart.py", RUN, "compute_total", None, vec![import]),
    ]);

    let (_, keys) = resolve_graph_recording(&mut graph);

    let edge = &graph.files["shop/cart.py"].edges[0];
    assert_bound(edge, &func_id("shop/pricing.py", "total"), Import);
    let consulted = &keys[&EdgeRef {
        path: "shop/cart.py".to_string(),
        index: 0,
    }];
    assert!(
        consulted.contains("name:python:__init__"),
        "an edit to the re-exporting file must re-resolve the edge: {consulted:?}"
    );
}

#[test]
fn a_csharp_alias_of_a_type_binds_a_static_call_on_it() {
    let widget: Symbols = &[(K::Module, &["App.Core"]), (K::Function, &["W", "Run"])];
    let strings: Symbols = &[
        (K::Module, &["App.Util"]),
        (K::Type, &["Strings"]),
        (K::Function, &["Strings", "Shout"]),
    ];
    let other: Symbols = &[
        (K::Module, &["App.Util"]),
        (K::Function, &["Text", "Shout"]),
    ];
    let alias = binding("App.Util.Strings", Some("Strings"), Some("S"));
    let edge = first_edge(
        vec![
            caller("src/Core/W.cs", widget, "Shout", Some("S"), vec![alias]),
            defining("src/Util/Strings.cs", strings),
            defining("src/Util/Text.cs", other),
        ],
        "src/Core/W.cs",
    );
    let shout = nested_id("src/Util/Strings.cs", K::Function, &["Strings", "Shout"]);
    assert_bound(&edge, &shout, Import);
}

#[test]
fn a_type_a_glob_import_brings_into_scope_is_a_receiver_and_a_value_is_not() {
    let widget: Symbols = &[
        (K::Module, &["app.core"]),
        (K::Function, &["W", "describe"]),
    ];
    let strings: Symbols = &[
        (K::Module, &["app.util"]),
        (K::Type, &["Strings"]),
        (K::Function, &["Strings", "clean"]),
    ];
    let files = |receiver: &str| {
        vec![
            caller(
                "src/app/core/W.java",
                widget,
                "clean",
                Some(receiver),
                vec![glob_binding("app.util")],
            ),
            defining("src/app/util/Strings.java", strings),
        ]
    };
    let clean = nested_id(
        "src/app/util/Strings.java",
        K::Function,
        &["Strings", "clean"],
    );

    let static_call = first_edge(files("Strings"), "src/app/core/W.java");
    assert_bound(&static_call, &clean, Import);

    let value_call = first_edge(files("strings"), "src/app/core/W.java");
    assert_candidates(&value_call, &[clean]);
}

#[test]
fn a_prototype_in_a_local_header_lifts_the_system_include_refusal() {
    let prototype = |from: &str, name: &str| unresolved_edge(from, References, name);
    let header = source_file(
        "include/shapes.h",
        &[],
        vec![prototype("include/shapes.h", "area")],
    );
    let includes = vec![glob_binding("<stdio.h>"), glob_binding("shapes.h")];
    let mut main = caller("src/main.c", RUN, "area", None, includes);
    let run = func_id("src/main.c", "run");
    main.edges
        .push(unresolved_edge(run.as_str(), Calls, "scale"));
    main.edges
        .push(unresolved_edge(run.as_str(), Calls, "printf"));
    main.edges.push(prototype("src/main.c", "scale"));
    let mut graph = graph_of_files(vec![
        header,
        main,
        source_file("src/shapes.c", &["area"], vec![]),
        source_file("src/util.c", &["scale", "printf"], vec![]),
    ]);

    resolve_graph(&mut graph);

    let edges = &graph.files["src/main.c"].edges;
    assert_bound(&edges[0], &func_id("src/shapes.c", "area"), UniqueName);
    assert_bound(&edges[1], &func_id("src/util.c", "scale"), UniqueName);
    assert_candidates(&edges[2], &[func_id("src/util.c", "printf")]);
}

#[test]
fn a_constructor_call_binds_the_constructor_and_never_the_file() {
    let widget: Symbols = &[(K::Type, &["Widget"]), (K::Function, &["Widget", "Widget"])];
    let test: Symbols = &[
        (K::Type, &["WidgetTest"]),
        (K::Function, &["WidgetTest", "t"]),
    ];
    let edge = first_edge(
        vec![
            caller("src/test/app/WidgetTest.java", test, "Widget", None, vec![]),
            defining("src/main/app/Widget.java", widget),
        ],
        "src/test/app/WidgetTest.java",
    );
    let constructor = nested_id(
        "src/main/app/Widget.java",
        K::Function,
        &["Widget", "Widget"],
    );
    assert_bound(&edge, &constructor, UniqueName);

    let edge = first_edge(
        vec![
            caller("src/app/Main.java", test, "Util", None, vec![]),
            defining("src/app/other/Util.java", &[]),
        ],
        "src/app/Main.java",
    );
    assert_eq!(
        edge.to, UNRESOLVED_TARGET,
        "a file is never called: {edge:#?}"
    );
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}

#[test]
fn a_constructor_never_makes_a_namesake_type_of_another_file_yield() {
    let with_ctor: Symbols = &[(K::Type, &["Widget"]), (K::Function, &["Widget", "Widget"])];
    let bare: Symbols = &[(K::Type, &["Widget"])];
    let main: Symbols = &[(K::Type, &["Main"]), (K::Function, &["Main", "run"])];
    let edge = first_edge(
        vec![
            caller("src/app/Main.java", main, "Widget", None, vec![]),
            defining("src/a/Widget.java", with_ctor),
            defining("src/b/Widget.java", bare),
        ],
        "src/app/Main.java",
    );
    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert!(
        edge.candidates
            .contains(&nested_id("src/b/Widget.java", K::Type, &["Widget"])),
        "edge: {edge:#?}"
    );
}

#[test]
fn an_out_of_line_constructor_makes_its_header_type_yield() {
    let main: Symbols = &[(K::Function, &["main"])];
    let ctor: Symbols = &[(K::Function, &["Widget", "Widget"])];
    let edge = first_edge(
        vec![
            caller("src/main.cpp", main, "Widget", None, vec![]),
            defining("include/widget.h", &[(K::Type, &["Widget"])]),
            defining("src/widget.cpp", ctor),
        ],
        "src/main.cpp",
    );
    let constructor = nested_id("src/widget.cpp", K::Function, &["Widget", "Widget"]);
    assert_bound(&edge, &constructor, UniqueName);
}

#[test]
fn a_self_call_binds_a_method_of_a_used_trait() {
    let widget: Symbols = &[(K::Type, &["Widget"]), (K::Function, &["Widget", "run"])];
    let mut file = caller("src/Widget.php", widget, "log", Some("$this"), vec![]);
    let class = nested_id("src/Widget.php", K::Type, &["Widget"]);
    file.edges
        .push(unresolved_edge(class, References, "Loggable"));
    let trait_symbols: Symbols = &[
        (K::Type, &["Loggable"]),
        (K::Function, &["Loggable", "log"]),
    ];
    let edge = first_edge(
        vec![
            file,
            defining("src/Loggable.php", trait_symbols),
            defining("src/Other.php", &[(K::Function, &["Other", "log"])]),
        ],
        "src/Widget.php",
    );
    let log = nested_id("src/Loggable.php", K::Function, &["Loggable", "log"]);
    assert_bound(&edge, &log, Receiver);
}
