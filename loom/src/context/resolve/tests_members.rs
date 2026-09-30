//! Which members a call can reach: a self call only the enclosing type's own
//! parts (and a PHP type's traits), a bare call no member at all where the
//! dialect's bare calls never reach members. Graphs are built by hand in the
//! shapes the extractors emit.

use super::fixtures::*;
use super::tests_rules::{assert_bound, assert_candidates, caller, defining, first_edge};
use crate::context::graph_store::FileEntry;
use crate::context::source_graph::EdgeProvenance::{Import, Receiver};
use crate::context::source_graph::SourceEdgeKind::References;
use crate::context::source_graph::{SourceEdge, SourceNodeKind as K, UNRESOLVED_TARGET};

const MAIN: Symbols<'static> = &[(K::Function, &["main"])];

fn assert_gap(edge: &SourceEdge) {
    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
}

#[test]
fn an_inherited_member_never_binds_a_namesake_types_member() {
    let widget: Symbols = &[
        (K::Module, &["app.a"]),
        (K::Type, &["Widget"]),
        (K::Function, &["Widget", "run"]),
    ];
    let base: Symbols = &[
        (K::Module, &["app.a"]),
        (K::Type, &["Base"]),
        (K::Function, &["Base", "reset"]),
    ];
    let namesake: Symbols = &[
        (K::Module, &["app.b"]),
        (K::Type, &["Widget"]),
        (K::Function, &["Widget", "reset"]),
    ];
    let edge = first_edge(
        vec![
            caller("app/a/Widget.java", widget, "reset", Some("this"), vec![]),
            defining("app/a/Base.java", base),
            defining("app/b/Widget.java", namesake),
        ],
        "app/a/Widget.java",
    );
    assert_candidates(
        &edge,
        &[
            nested_id("app/a/Base.java", K::Function, &["Base", "reset"]),
            nested_id("app/b/Widget.java", K::Function, &["Widget", "reset"]),
        ],
    );
}

#[test]
fn a_partial_class_joins_its_parts_within_its_namespace_only() {
    let widget: Symbols = &[
        (K::Module, &["App.A"]),
        (K::Type, &["App.A", "Widget"]),
        (K::Function, &["App.A", "Widget", "Run"]),
    ];
    let part: Symbols = &[
        (K::Module, &["App.A"]),
        (K::Type, &["App.A", "Widget"]),
        (K::Function, &["App.A", "Widget", "Save"]),
    ];
    let namesake: Symbols = &[
        (K::Module, &["App.B"]),
        (K::Type, &["App.B", "Widget"]),
        (K::Function, &["App.B", "Widget", "Reset"]),
    ];
    let files = |call: &str| -> Vec<FileEntry> {
        vec![
            caller("src/A/Widget.cs", widget, call, Some("this"), vec![]),
            defining("src/A/WidgetSave.cs", part),
            defining("src/B/Widget.cs", namesake),
        ]
    };
    let save = nested_id(
        "src/A/WidgetSave.cs",
        K::Function,
        &["App.A", "Widget", "Save"],
    );
    let reset = nested_id(
        "src/B/Widget.cs",
        K::Function,
        &["App.B", "Widget", "Reset"],
    );

    assert_bound(
        &first_edge(files("Save"), "src/A/Widget.cs"),
        &save,
        Receiver,
    );
    assert_candidates(&first_edge(files("Reset"), "src/A/Widget.cs"), &[reset]);
}

#[test]
fn an_impl_block_joins_its_type_only_within_its_crate() {
    let declared: Symbols = &[(K::Type, &["W"]), (K::Function, &["W", "a"])];
    let extended: Symbols = &[(K::Implementation, &["W"]), (K::Function, &["W", "b"])];
    let crates = || -> Vec<FileEntry> {
        vec![
            defining("crates/a/src/lib.rs", &[]),
            defining("crates/b/src/lib.rs", &[]),
            caller("crates/a/src/w.rs", declared, "b", Some("self"), vec![]),
            defining("crates/b/src/w_ext.rs", extended),
        ]
    };
    let foreign = nested_id("crates/b/src/w_ext.rs", K::Function, &["W", "b"]);
    assert_candidates(&first_edge(crates(), "crates/a/src/w.rs"), &[foreign]);

    let mut files = crates();
    files.push(defining("crates/a/src/w_ext.rs", extended));
    let own = nested_id("crates/a/src/w_ext.rs", K::Function, &["W", "b"]);
    assert_bound(&first_edge(files, "crates/a/src/w.rs"), &own, Receiver);
}

#[test]
fn a_tsx_class_referencing_a_component_uses_no_trait() {
    let panel: Symbols = &[(K::Type, &["Panel"]), (K::Function, &["Panel", "render"])];
    let mut file = caller("src/Panel.tsx", panel, "open", Some("this"), vec![]);
    let class = nested_id("src/Panel.tsx", K::Type, &["Panel"]);
    file.edges
        .push(unresolved_edge(class, References, "Dialog"));
    let dialog: Symbols = &[(K::Type, &["Dialog"]), (K::Function, &["Dialog", "open"])];
    let edge = first_edge(
        vec![file, defining("src/Dialog.tsx", dialog)],
        "src/Panel.tsx",
    );
    let open = nested_id("src/Dialog.tsx", K::Function, &["Dialog", "open"]);
    assert_candidates(&edge, &[open]);
}

#[test]
fn a_bare_go_call_never_binds_a_method() {
    let widget: Symbols = &[(K::Type, &["Widget"]), (K::Function, &["Widget", "run"])];
    let beside_a_function = first_edge(
        vec![
            defining("pkg/a.go", widget),
            source_file("pkg/b.go", &["run"], vec![]),
            caller("pkg/c.go", MAIN, "run", None, vec![]),
        ],
        "pkg/c.go",
    );
    assert_bound(&beside_a_function, &func_id("pkg/b.go", "run"), Import);

    let method_only = first_edge(
        vec![
            defining("pkg/a.go", widget),
            caller("other/c.go", MAIN, "run", None, vec![]),
        ],
        "other/c.go",
    );
    assert_gap(&method_only);
}

#[test]
fn a_go_builtin_never_binds_a_method_of_a_named_non_struct_type() {
    // `type stack []int` with `func (s *stack) len() int`, once with the type
    // node the extractor emits and once with the type's file lost to a parse
    // error: the method is a member either way, so `len` stays the builtin.
    let typed: Symbols = &[(K::Type, &["stack"]), (K::Function, &["stack", "len"])];
    let untyped: Symbols = &[(K::Function, &["stack", "len"])];
    for stack in [typed, untyped] {
        for caller_path in ["pkg/count.go", "other/count.go"] {
            let edge = first_edge(
                vec![
                    defining("pkg/stack.go", stack),
                    caller(caller_path, MAIN, "len", None, vec![]),
                ],
                caller_path,
            );
            assert_gap(&edge);
        }
    }
}

#[test]
fn a_self_call_never_binds_a_member_of_a_nested_namesake_type() {
    // Python `class A: class Meta: def ordering(self)` beside
    // `class B: class Meta(Base): def run(self): self.ordering()`.
    let models: Symbols = &[
        (K::Type, &["A"]),
        (K::Type, &["A", "Meta"]),
        (K::Function, &["A", "Meta", "ordering"]),
        (K::Type, &["B"]),
        (K::Type, &["B", "Meta"]),
        (K::Function, &["B", "Meta", "stop"]),
        (K::Function, &["B", "Meta", "run"]),
    ];
    let edge = |member: &str| {
        let file = caller("models.py", models, member, Some("self"), vec![]);
        first_edge(vec![file], "models.py")
    };
    let ordering = nested_id("models.py", K::Function, &["A", "Meta", "ordering"]);
    assert_candidates(&edge("ordering"), &[ordering]);

    let stop = nested_id("models.py", K::Function, &["B", "Meta", "stop"]);
    assert_bound(&edge("stop"), &stop, Receiver);
}

#[test]
fn dropping_a_member_records_its_owner_name() {
    let widget: Symbols = &[(K::Type, &["Widget"]), (K::Function, &["Widget", "run"])];
    let mut graph = graph_of_files(vec![
        defining("pkg/a.go", widget),
        caller("other/c.go", MAIN, "run", None, vec![]),
    ]);

    let (_, keys) = super::resolve_graph_recording(&mut graph);

    let edge = super::EdgeRef {
        path: "other/c.go".to_string(),
        index: 0,
    };
    assert!(
        keys[&edge].contains("name:go:Widget"),
        "a change to the owner type must re-resolve the edge: {:?}",
        keys[&edge]
    );
}

#[test]
fn a_bare_rust_call_never_binds_a_method() {
    let parse: Symbols = &[(K::Implementation, &["W"]), (K::Function, &["W", "parse"])];
    for imports in [vec![], vec![glob_binding("crate::w")]] {
        let edge = first_edge(
            vec![
                defining("src/lib.rs", &[]),
                defining("src/w.rs", parse),
                caller("src/app.rs", MAIN, "parse", None, imports),
            ],
            "src/app.rs",
        );
        assert_gap(&edge);
    }
}
