//! One test per resolution rule and refusal, across dialects. Graphs are built
//! by hand in the shapes the extractors emit, so dialects without an extractor
//! in this build are covered too.

use super::fixtures::*;
use super::*;
use crate::context::graph_store::FileEntry;
use crate::context::source_graph::EdgeProvenance::{Import, Receiver, UniqueName};
use crate::context::source_graph::{
    ImportBinding, SourceNodeKind as K, MAX_CANDIDATES, UNRESOLVED_TARGET,
};

/// A file whose last symbol makes one unresolved call, on `receiver` if given.
pub(super) fn caller(
    path: &str,
    symbols: Symbols,
    symbol: &str,
    receiver: Option<&str>,
    imports: Vec<ImportBinding>,
) -> FileEntry {
    let (kind, scope) = symbols.last().expect("a calling symbol");
    let mut edge = unresolved_edge(nested_id(path, *kind, scope), SourceEdgeKind::Calls, symbol);
    edge.receiver = receiver.map(str::to_string);
    dialect_file(path, symbols, vec![edge], imports)
}

/// A file holding `symbols` and nothing else.
pub(super) fn defining(path: &str, symbols: Symbols) -> FileEntry {
    dialect_file(path, symbols, vec![], vec![])
}

/// The first edge of `path` once the whole graph is resolved.
pub(super) fn first_edge(files: Vec<FileEntry>, path: &str) -> SourceEdge {
    let mut graph = graph_of_files(files);
    resolve_graph(&mut graph);
    graph.files[path].edges[0].clone()
}

pub(super) fn assert_bound(edge: &SourceEdge, to: &str, provenance: EdgeProvenance) {
    assert_eq!(edge.to, to, "edge: {edge:#?}");
    assert_eq!(edge.provenance, provenance, "edge: {edge:#?}");
}

pub(super) fn assert_candidates(edge: &SourceEdge, candidates: &[String]) {
    assert_eq!(edge.to, UNRESOLVED_TARGET, "edge: {edge:#?}");
    assert_eq!(edge.provenance, EdgeProvenance::Syntax, "edge: {edge:#?}");
    assert_eq!(edge.candidates, candidates, "edge: {edge:#?}");
}

const RUN: Symbols<'static> = &[(K::Function, &["run"])];

#[test]
fn typescript_alias_binds_the_aliased_definition() {
    let alias = binding("./util", Some("parse"), Some("p"));
    let edge = first_edge(
        vec![
            source_file("src/util.ts", &["parse"], vec![]),
            source_file("src/other.ts", &["parse"], vec![]),
            caller("src/main.ts", RUN, "p", None, vec![alias]),
        ],
        "src/main.ts",
    );
    assert_bound(&edge, &func_id("src/util.ts", "parse"), Import);
}

#[test]
fn python_module_alias_binds_a_member_call() {
    let alias = binding("a.b", None, Some("c"));
    let edge = first_edge(
        vec![
            source_file("a/b.py", &["f"], vec![]),
            source_file("other.py", &["f"], vec![]),
            caller("main.py", RUN, "f", Some("c"), vec![alias]),
        ],
        "main.py",
    );
    assert_bound(&edge, &func_id("a/b.py", "f"), Import);
}

#[test]
fn go_call_binds_within_its_package() {
    let edge = first_edge(
        vec![
            caller("pkg/util/a.go", RUN, "helper", None, vec![]),
            source_file("pkg/util/b.go", &["helper"], vec![]),
            source_file("pkg/other/c.go", &["helper"], vec![]),
        ],
        "pkg/util/a.go",
    );
    assert_bound(&edge, &func_id("pkg/util/b.go", "helper"), Import);
}

#[test]
fn java_call_binds_a_method_of_a_same_package_class() {
    let main: Symbols = &[(K::Module, &["a.b"]), (K::Function, &["Main", "run"])];
    let base: Symbols = &[(K::Module, &["a.b"]), (K::Function, &["Base", "helper"])];
    let other: Symbols = &[(K::Module, &["c"]), (K::Function, &["Other", "helper"])];
    let edge = first_edge(
        vec![
            caller("src/a/b/Main.java", main, "helper", None, vec![]),
            defining("src/a/b/Base.java", base),
            defining("src/c/Other.java", other),
        ],
        "src/a/b/Main.java",
    );
    let helper = nested_id("src/a/b/Base.java", K::Function, &["Base", "helper"]);
    assert_bound(&edge, &helper, Import);
}

#[test]
fn csharp_using_binds_a_type_of_the_namespace() {
    let program: Symbols = &[(K::Module, &["App"]), (K::Function, &["App", "P", "Main"])];
    let lib: Symbols = &[
        (K::Module, &["Lib.Util"]),
        (K::Type, &["Lib.Util", "Helper"]),
    ];
    let other: Symbols = &[(K::Module, &["Other"]), (K::Type, &["Other", "Helper"])];
    let using = glob_binding("Lib.Util");
    let edge = first_edge(
        vec![
            caller("src/App/P.cs", program, "Helper", None, vec![using]),
            defining("src/Lib/Helper.cs", lib),
            defining("src/Other/Helper.cs", other),
        ],
        "src/App/P.cs",
    );
    let helper = nested_id("src/Lib/Helper.cs", K::Type, &["Lib.Util", "Helper"]);
    assert_bound(&edge, &helper, Import);
}

#[test]
fn ruby_self_call_binds_a_method_of_the_reopened_class() {
    let widget: Symbols = &[(K::Type, &["Widget"]), (K::Function, &["Widget", "run"])];
    let reopened: Symbols = &[(K::Type, &["Widget"]), (K::Function, &["Widget", "helper"])];
    let gadget: Symbols = &[(K::Type, &["Gadget"]), (K::Function, &["Gadget", "helper"])];
    let edge = first_edge(
        vec![
            caller("lib/widget.rb", widget, "helper", Some("self"), vec![]),
            defining("lib/widget_ext.rb", reopened),
            defining("lib/gadget.rb", gadget),
        ],
        "lib/widget.rb",
    );
    let helper = nested_id("lib/widget_ext.rb", K::Function, &["Widget", "helper"]);
    assert_bound(&edge, &helper, Receiver);
}

#[test]
fn a_self_call_matches_the_enclosing_type_by_its_own_name() {
    let inline: Symbols = &[
        (K::Type, &["example", "Widget"]),
        (K::Function, &["example", "Widget", "a"]),
    ];
    let top: Symbols = &[
        (K::Implementation, &["Widget"]),
        (K::Function, &["Widget", "b"]),
    ];
    let edge = first_edge(
        vec![
            caller("src/w.rs", inline, "b", Some("self"), vec![]),
            defining("src/w_ext.rs", top),
        ],
        "src/w.rs",
    );
    assert_bound(
        &edge,
        &nested_id("src/w_ext.rs", K::Function, &["Widget", "b"]),
        Receiver,
    );
}

#[test]
fn php_use_alias_binds_a_static_call() {
    let main: Symbols = &[(K::Type, &["Main"]), (K::Function, &["Main", "run"])];
    let parser: Symbols = &[
        (K::Module, &["App.Util"]),
        (K::Function, &["Parser", "parse"]),
    ];
    let alias = binding("App\\Util\\Parser", Some("Parser"), Some("P"));
    let edge = first_edge(
        vec![
            caller("src/App/Main.php", main, "parse", Some("P"), vec![alias]),
            defining("src/App/Util/Parser.php", parser),
            defining("src/App/Other.php", &[(K::Function, &["Other", "parse"])]),
        ],
        "src/App/Main.php",
    );
    let parse = nested_id("src/App/Util/Parser.php", K::Function, &["Parser", "parse"]);
    assert_bound(&edge, &parse, Import);
}

#[test]
fn c_include_binds_by_unique_name_unless_a_system_include_refuses() {
    let files = |includes: Vec<ImportBinding>| {
        vec![
            source_file("src/x.h", &[], vec![]),
            source_file("src/x.c", &["f"], vec![]),
            caller("src/main.c", RUN, "f", None, includes),
        ]
    };
    let f = func_id("src/x.c", "f");

    let local = first_edge(files(vec![glob_binding("x.h")]), "src/main.c");
    assert_bound(&local, &f, UniqueName);

    let system = vec![glob_binding("x.h"), glob_binding("<stdio.h>")];
    assert_candidates(&first_edge(files(system), "src/main.c"), &[f]);
}

#[test]
fn a_dynamic_receiver_lists_a_unique_namesake_without_binding_it() {
    let edge = first_edge(
        vec![
            defining(
                "src/a.ts",
                &[(K::Type, &["A"]), (K::Function, &["A", "save"])],
            ),
            caller("src/run.ts", RUN, "save", Some("obj"), vec![]),
        ],
        "src/run.ts",
    );
    assert_candidates(&edge, &[nested_id("src/a.ts", K::Function, &["A", "save"])]);
}

#[test]
fn nine_definitions_leave_no_candidates() {
    let with_definitions = |count: usize| {
        let mut files: Vec<FileEntry> = (0..count)
            .map(|n| source_file(&format!("src/d{n}.rs"), &["helper"], vec![]))
            .collect();
        files.push(caller("src/app.rs", RUN, "helper", None, vec![]));
        first_edge(files, "src/app.rs")
    };

    let at_cap = with_definitions(MAX_CANDIDATES);
    assert_eq!(at_cap.candidates.len(), MAX_CANDIDATES, "edge: {at_cap:#?}");
    assert_candidates(&with_definitions(MAX_CANDIDATES + 1), &[]);
}

#[test]
fn a_name_never_binds_across_families() {
    let edge = first_edge(
        vec![
            caller("src/app.ts", RUN, "helper", None, vec![]),
            source_file("src/lib.rs", &["helper"], vec![]),
            source_file("src/tool.py", &["helper"], vec![]),
        ],
        "src/app.ts",
    );
    assert_candidates(&edge, &[]);

    let edge = first_edge(
        vec![
            caller("src/app.ts", RUN, "helper", None, vec![]),
            source_file("src/util.js", &["helper"], vec![]),
        ],
        "src/app.ts",
    );
    assert_bound(&edge, &func_id("src/util.js", "helper"), UniqueName);
}

#[test]
fn an_external_named_import_refuses_unique_name() {
    let import = binding("lodash", Some("debounce"), None);
    let edge = first_edge(
        vec![
            source_file("src/util.ts", &["debounce"], vec![]),
            caller("src/main.ts", RUN, "debounce", None, vec![import]),
        ],
        "src/main.ts",
    );
    assert_candidates(&edge, &[func_id("src/util.ts", "debounce")]);
}

#[test]
fn a_crate_glob_import_binds_the_name_it_exports() {
    let edge = first_edge(
        vec![
            source_file("src/lib.rs", &[], vec![]),
            source_file("src/x.rs", &["helper"], vec![]),
            source_file("src/y.rs", &["helper"], vec![]),
            caller(
                "src/app.rs",
                RUN,
                "helper",
                None,
                vec![glob_binding("crate::x")],
            ),
        ],
        "src/app.rs",
    );
    assert_bound(&edge, &func_id("src/x.rs", "helper"), Import);
}

#[test]
fn a_relative_glob_the_conventions_cannot_place_refuses_nothing() {
    let edge = first_edge(
        vec![
            source_file("src/b.rs", &["helper"], vec![]),
            caller(
                "src/a/tests.rs",
                RUN,
                "helper",
                None,
                vec![glob_binding("super")],
            ),
        ],
        "src/a/tests.rs",
    );
    assert_bound(&edge, &func_id("src/b.rs", "helper"), UniqueName);
}

/// `caller_path` imports `helper` from `spec`, which names no file; `other` holds
/// the only definition. The edge is refused, never bound by unique name.
fn assert_unplaced_import_refuses(caller_path: &str, spec: &str, other: &str) {
    let import = binding(spec, Some("helper"), None);
    let edge = first_edge(
        vec![
            source_file(other, &["helper"], vec![]),
            caller(caller_path, RUN, "helper", None, vec![import]),
        ],
        caller_path,
    );
    assert_candidates(&edge, &[func_id(other, "helper")]);
}

#[test]
fn an_unplaced_path_style_import_is_external_and_refuses() {
    assert_unplaced_import_refuses("src/main.ts", "./helper.vue", "src/other.ts");
    assert_unplaced_import_refuses("main.py", ".native", "other.py");
}

#[test]
fn a_qualifier_an_import_binds_resolves_through_the_import() {
    let import = binding("crate::context::codex", Some("codex"), None);
    let edge = first_edge(
        vec![
            source_file("src/lib.rs", &[], vec![]),
            source_file("src/context/codex.rs", &["run"], vec![]),
            source_file("src/other.rs", &["run"], vec![]),
            caller("src/app.rs", RUN, "codex::run", None, vec![import]),
        ],
        "src/app.rs",
    );
    assert_bound(&edge, &func_id("src/context/codex.rs", "run"), Import);
}

#[test]
fn a_qualified_spelling_is_matched_before_import_bindings() {
    let import = binding("crate::gadget::Widget", Some("Widget"), None);
    let edge = first_edge(
        vec![
            defining("src/widget.rs", &[(K::Function, &["Widget", "new"])]),
            source_file("src/gadget.rs", &["new"], vec![]),
            caller("src/app.rs", RUN, "Widget::new", None, vec![import]),
        ],
        "src/app.rs",
    );
    let new = nested_id("src/widget.rs", K::Function, &["Widget", "new"]);
    assert_bound(&edge, &new, Import);
}

#[test]
fn a_single_type_import_wins_over_the_package() {
    let main: Symbols = &[(K::Type, &["Main"]), (K::Function, &["Main", "run"])];
    let import = binding("c.Helper", Some("Helper"), None);
    let edge = first_edge(
        vec![
            caller("src/a/Main.java", main, "Helper", None, vec![import]),
            defining("src/a/Helper.java", &[(K::Type, &["Helper"])]),
            defining("src/c/Helper.java", &[(K::Type, &["Helper"])]),
        ],
        "src/a/Main.java",
    );
    assert_bound(
        &edge,
        &nested_id("src/c/Helper.java", K::Type, &["Helper"]),
        Import,
    );
}

#[test]
fn a_lone_candidate_that_is_the_caller_resolves_nothing() {
    let edge = first_edge(
        vec![caller("src/main.ts", RUN, "run", Some("obj"), vec![])],
        "src/main.ts",
    );
    assert_candidates(&edge, &[]);
}
