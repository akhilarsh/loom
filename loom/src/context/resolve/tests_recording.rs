//! The recording API: the keys each resolution consulted, the keys a file's
//! nodes answer to, partial resolution, and stats as a pure count.

use std::collections::{BTreeMap, BTreeSet};

use super::fixtures::*;
use super::*;
use crate::context::graph_store::FileEntry;
use crate::context::source_graph::SourceEdgeKind::{Calls, Contains, Imports};
use crate::context::source_graph::SourceNodeKind as K;

/// A graph exercising an alias, a dynamic receiver, an import edge, a self
/// receiver, a unique name, a package-scope call and a containment edge, over
/// file, type, implementation, module and function nodes.
fn mixed_graph() -> ResolvedGraph {
    let mut files = typescript_files();
    files.extend(rust_files());
    files.extend(java_files());
    graph_of_files(files)
}

/// An aliased call, a call on a dynamic receiver and an import statement.
fn typescript_files() -> Vec<FileEntry> {
    let run = func_id("src/main.ts", "run");
    let edges = vec![
        unresolved_edge(run.as_str(), Calls, "p"),
        unresolved_edge(run.as_str(), Calls, "save").with_receiver("obj"),
        unresolved_edge("src/main.ts", Imports, "./util"),
    ];
    let alias = binding("./util", Some("parse"), Some("p"));
    vec![
        source_file("src/util.ts", &["parse", "save"], vec![]),
        dialect_file(
            "src/main.ts",
            &[(K::Function, &["run"])],
            edges,
            vec![alias],
        ),
    ]
}

/// A self-receiver call into a second `impl` block, a unique-name call and a
/// containment edge.
fn rust_files() -> Vec<FileEntry> {
    let a = nested_id("src/w.rs", K::Function, &["W", "a"]);
    let w = nested_id("src/w.rs", K::Type, &["W"]);
    let edges = vec![
        unresolved_edge(a.as_str(), Calls, "b").with_receiver("self"),
        unresolved_edge(a.as_str(), Calls, "helper"),
        local_edge("src/w.rs", w, Contains, "W"),
    ];
    let declared: Symbols = &[
        (K::Type, &["W"]),
        (K::Implementation, &["W"]),
        (K::Function, &["W", "a"]),
    ];
    let extended: Symbols = &[
        (K::Implementation, &["W"]),
        (K::Function, &["W", "b"]),
        (K::Function, &["helper"]),
    ];
    vec![
        dialect_file("src/w.rs", declared, edges, vec![]),
        dialect_file("src/w_ext.rs", extended, vec![], vec![]),
    ]
}

/// A call bound through the package directory.
fn java_files() -> Vec<FileEntry> {
    let main = nested_id("src/a/Main.java", K::Function, &["Main", "run"]);
    let calling: Symbols = &[
        (K::Module, &["a"]),
        (K::Type, &["Main"]),
        (K::Function, &["Main", "run"]),
    ];
    let base: Symbols = &[(K::Module, &["a"]), (K::Function, &["Base", "helper"])];
    let edges = vec![unresolved_edge(main, Calls, "helper")];
    vec![
        dialect_file("src/a/Main.java", calling, edges, vec![]),
        dialect_file("src/a/Base.java", base, vec![], vec![]),
    ]
}

fn edge_ref(path: &str, index: usize) -> EdgeRef {
    EdgeRef {
        path: path.to_string(),
        index,
    }
}

fn strings(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn an_import_bound_edge_records_its_pathset_and_name_keys() {
    let (_, keys) = resolve_graph_recording(&mut mixed_graph());

    assert_eq!(
        keys[&edge_ref("src/main.ts", 0)],
        strings(&["name:ecmascript:parse", "pathset:ecmascript"])
    );
    assert_eq!(
        keys[&edge_ref("src/a/Main.java", 0)],
        strings(&["name:java:helper", "pathset:java"]),
        "a package-scope lookup depends on which files exist"
    );
}

#[test]
fn touched_keys_list_an_entrys_names_and_namespaces() {
    let entry = dialect_file(
        "src/a/b/Main.java",
        &[
            (K::Module, &["a.b"]),
            (K::Type, &["Main"]),
            (K::Function, &["Main", "run"]),
        ],
        vec![],
        vec![],
    );
    let names = [
        "name:java:Main",
        "name:java:b",
        "name:java:java",
        "name:java:run",
        "ns:java:a.b",
    ];

    assert_eq!(touched_keys(&entry, false), strings(&names));
    let mut added = strings(&names);
    added.insert("pathset:java".to_string());
    assert_eq!(touched_keys(&entry, true), added);
}

#[test]
fn index_buckets_map_to_touched_keys() {
    let graph = mixed_graph();
    let index = SymbolIndex::build(&graph);
    let mut kinds = BTreeSet::new();

    for (family, spelling, ids) in index.buckets() {
        let name = spelling.rsplit([':', '.']).next().unwrap_or(spelling);
        let key = format!("name:{family}:{name}");
        for id in ids {
            let (entry, node) = graph
                .files
                .values()
                .find_map(|entry| Some((entry, entry.nodes.iter().find(|n| n.id == *id)?)))
                .expect("every indexed id is a node of the graph");
            kinds.insert(node.kind.as_str());
            assert!(
                touched_keys(entry, false).contains(&key),
                "{id} is indexed under {spelling}, but its file does not touch {key}"
            );
        }
    }
    assert_eq!(
        kinds,
        BTreeSet::from(["file", "function", "implementation", "module", "type"])
    );
}

#[test]
fn resolve_edges_matches_full_resolution_on_selected_edges() {
    let mut cold = mixed_graph();
    let (_, cold_keys) = resolve_graph_recording(&mut cold);
    let selected: BTreeSet<EdgeRef> = cold
        .files
        .iter()
        .flat_map(|(path, entry)| {
            entry
                .edges
                .iter()
                .enumerate()
                .filter_map(move |(index, edge)| {
                    let decided = edge.kind != Contains
                        && (!edge.is_unresolved() || !edge.candidates.is_empty());
                    decided.then(|| edge_ref(path, index))
                })
        })
        .collect();
    assert_eq!(selected.len(), 6, "every resolver outcome in the fixture");

    let mut warm = cold.clone();
    for edge in &selected {
        warm.files.get_mut(&edge.path).expect("file").edges[edge.index].unbind();
    }
    assert_ne!(warm, cold);
    let keys = resolve_edges(&mut warm, &selected);

    assert_eq!(warm, cold);
    assert_eq!(keys.keys().cloned().collect::<BTreeSet<_>>(), selected);
    for (edge, consulted) in &keys {
        assert_eq!(consulted, &cold_keys[edge], "keys of {edge:?}");
    }
}

#[test]
fn resolve_edges_over_a_subset_leaves_other_edges_untouched() {
    let mut graph = mixed_graph();
    let before = graph.clone();
    let chosen = edge_ref("src/w.rs", 0);

    let keys = resolve_edges(&mut graph, &BTreeSet::from([chosen.clone()]));

    assert_eq!(keys.keys().collect::<Vec<_>>(), [&chosen]);
    for (path, entry) in &graph.files {
        for (index, edge) in entry.edges.iter().enumerate() {
            if edge_ref(path, index) == chosen {
                assert_eq!(edge.provenance, EdgeProvenance::Receiver, "{edge:#?}");
            } else {
                assert_eq!(edge, &before.files[path].edges[index]);
            }
        }
    }
}

#[test]
fn stats_equal_resolution_stats_after_resolving() {
    let mut graph = mixed_graph();

    let stats = resolve_graph(&mut graph);

    assert_eq!(stats, resolution_stats(&graph));
    assert_eq!(stats, resolve_graph_recording(&mut mixed_graph()).0);
    assert_eq!(stats, expected_stats(5, 1, 1));
    let by_provenance = [
        ("import", 3),
        ("receiver", 1),
        ("syntax", 1),
        ("unique-name", 1),
    ];
    let by_provenance: BTreeMap<String, usize> = by_provenance
        .into_iter()
        .map(|(provenance, count)| (provenance.to_string(), count))
        .collect();
    assert_eq!(stats.by_provenance, by_provenance);
    let json = serde_json::to_string(&stats).expect("stats serialize");
    let parsed: ResolutionStats = serde_json::from_str(&json).expect("stats deserialize");
    assert_eq!(parsed, stats);
}

#[test]
fn an_extraction_candidate_set_is_untouched_and_records_no_keys() {
    let run = func_id("src/app.rs", "run");
    let rivals = vec![
        nested_id("src/app.rs", K::Function, &["a", "helper"]),
        nested_id("src/app.rs", K::Function, &["b", "helper"]),
    ];
    let ambiguous = unresolved_edge(run, Calls, "helper").with_candidates(rivals);
    let mut graph = graph_of_files(vec![
        dialect_file(
            "src/app.rs",
            &[
                (K::Function, &["run"]),
                (K::Function, &["a", "helper"]),
                (K::Function, &["b", "helper"]),
            ],
            vec![ambiguous.clone()],
            vec![],
        ),
        source_file("src/other.rs", &["helper"], vec![]),
    ]);

    let (stats, keys) = resolve_graph_recording(&mut graph);

    assert_eq!(graph.files["src/app.rs"].edges[0], ambiguous);
    assert!(keys.is_empty(), "{keys:?}");
    assert_eq!(stats, expected_stats(0, 1, 1));
    let selected = BTreeSet::from([edge_ref("src/app.rs", 0)]);
    assert!(resolve_edges(&mut graph, &selected).is_empty());
    assert_eq!(graph.files["src/app.rs"].edges[0], ambiguous);
}
