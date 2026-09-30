//! Relink equals the cold build, byte for byte in canonical JSON: a seeded
//! edit sequence, extraction-time ambiguity, a namespace change, the header,
//! and every identity change.

use std::collections::{BTreeMap, BTreeSet};

use super::identity::extractor_digest;
use super::tests::{
    assert_relink_equals_cold, call, extracted, extracted_with, identity, A_HELPER, A_RS, B_RS,
    C_RS,
};
use super::*;
use crate::context::extract::{registry, BoxedExtractor};
use crate::context::graph_store::FileEntry;
use crate::context::resolve::fixtures::*;
use crate::context::source_graph::SourceEdgeKind::Calls;
use crate::context::source_graph::SourceNodeKind as K;
use crate::context::source_graph::{EdgeProvenance, FileCoverage};

const MAIN_JAVA: (&str, &str) = (
    "src/a/Main.java",
    "package a;\n\nclass Main {\n    void run() {\n        helper();\n    }\n}\n",
);

/// `src/app.rs` with a call extraction left ambiguous between two same-file
/// `helper`s, a caller in `src/user.rs`, and nine namesake files; `edited`
/// names the namesake whose entry changed.
fn ambiguity_graph(edited: Option<usize>) -> ResolvedGraph {
    let rivals = vec![
        nested_id("src/app.rs", K::Function, &["a", "helper"]),
        nested_id("src/app.rs", K::Function, &["b", "helper"]),
    ];
    let run = func_id("src/app.rs", "run");
    let ambiguous = unresolved_edge(run, Calls, "helper").with_candidates(rivals);
    let app_symbols: Symbols = &[
        (K::Function, &["run"]),
        (K::Function, &["a", "helper"]),
        (K::Function, &["b", "helper"]),
    ];
    let user_call = unresolved_edge(func_id("src/user.rs", "main"), Calls, "helper");
    let mut entries = vec![
        dialect_file("src/app.rs", app_symbols, vec![ambiguous], vec![]),
        source_file("src/user.rs", &["main"], vec![user_call]),
    ];
    for n in 0..9 {
        let path = format!("src/n{n}.rs");
        let entry = if edited == Some(n) {
            FileEntry {
                content_hash: format!("sha256:n{n}-edited"),
                ..source_file(&path, &["helper", "extra"], vec![])
            }
        } else {
            source_file(&path, &["helper"], vec![])
        };
        entries.push(entry);
    }
    graph_of_files(entries)
}

#[test]
fn same_file_ambiguity_survives_relink() {
    let extraction = ambiguity_graph(None).files["src/app.rs"].edges[0].clone();
    let v0 = build_cold(ambiguity_graph(None), identity("r0"));
    assert_eq!(v0.graph.files["src/app.rs"].edges[0], extraction);

    let next = ambiguity_graph(Some(3));
    let v1 = assert_relink_equals_cold(&v0, next, identity("r1"), "namesake edited");

    let app_call = &v1.graph.files["src/app.rs"].edges[0];
    assert_eq!(
        app_call, &extraction,
        "the extraction-time candidate set stays"
    );
    let consulted: Vec<&str> = v1
        .deps
        .edges("name:rust:helper")
        .map(|edge| edge.path.as_str())
        .collect();
    assert_eq!(
        consulted,
        ["src/user.rs"],
        "only the resolver's edge records keys"
    );
}

/// C# `App.cs` calls `Helper` under `using A.B;`, `Util.cs` defines the only
/// `Helper`, and `Ns.cs` declares namespace `A.B` once `declares` is set.
fn csharp_graph(declares: bool) -> ResolvedGraph {
    let call = unresolved_edge(func_id("src/App.cs", "Run"), Calls, "Helper");
    let using = vec![glob_binding("A.B")];
    let app = dialect_file("src/App.cs", &[(K::Function, &["Run"])], vec![call], using);
    let ns = if declares {
        let symbols: Symbols = &[(K::Module, &["A.B"]), (K::Function, &["Other"])];
        FileEntry {
            content_hash: "sha256:ns-declares-a-b".to_string(),
            ..dialect_file("src/Ns.cs", symbols, vec![], vec![])
        }
    } else {
        source_file("src/Ns.cs", &["Other"], vec![])
    };
    let util = source_file("src/Util.cs", &["Helper"], vec![]);
    graph_of_files(vec![app, util, ns])
}

#[test]
fn namespace_change_reselects_import() {
    let v0 = build_cold(csharp_graph(false), identity("r0"));
    let (app_call, edge) = call(&v0, "src/App.cs", "Helper");
    assert!(
        edge.is_unresolved(),
        "an external using refuses the unique name: {edge:#?}"
    );
    assert!(v0.deps.edges("ns:csharp:A.B").any(|edge| *edge == app_call));

    let v1 = assert_relink_equals_cold(&v0, csharp_graph(true), identity("r1"), "namespace");

    let edge = call(&v1, "src/App.cs", "Helper").1;
    assert_eq!(edge.to, func_id("src/Util.cs", "Helper"));
    assert_eq!(edge.provenance, EdgeProvenance::UniqueName);
}

#[test]
fn relink_takes_header_from_next() {
    let v0 = build_cold(extracted("r0", &[A_RS, B_RS]), identity("r0"));
    let mut next = extracted("r1", &[A_RS, B_RS, C_RS]);
    next.overlaid = BTreeSet::from(["src/c.rs".to_string()]);

    let relinked = assert_relink_equals_cold(&v0, next.clone(), identity("r1"), "header");

    assert_eq!(relinked.graph.base_revision, "r1");
    assert_eq!(relinked.graph.overlaid, next.overlaid);
}

/// The entry an unreadable file gets: no hash, no graph, the cause as detail.
pub(super) fn unreadable(cause: &str) -> FileEntry {
    FileEntry {
        content_hash: String::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        coverage: FileCoverage::LexicalOnly {
            detail: format!("unreadable: {cause}"),
        },
        imports: Vec::new(),
    }
}

#[test]
fn a_file_unreadable_on_both_sides_takes_its_next_entry() {
    let locked = "src/locked.rs".to_string();
    let mut before = extracted("r0", &[A_RS, B_RS]);
    before
        .files
        .insert(locked.clone(), unreadable("permission denied"));
    let v0 = build_cold(before, identity("r0"));
    let mut next = extracted("r1", &[A_RS, B_RS]);
    next.files
        .insert(locked.clone(), unreadable("symlink refused"));

    let v1 = assert_relink_equals_cold(&v0, next, identity("r1"), "unreadable cause changed");

    assert_eq!(v1.graph.files[&locked], unreadable("symlink refused"));
}

/// Relink a view built under another identity: it equals the cold build, and
/// its stats and bindings come from `files` as extracted now.
fn assert_identity_change_is_cold(previous: ResolvedView, files: &[(&str, &str)], context: &str) {
    let next = extracted("r1", files);
    let relinked = assert_relink_equals_cold(&previous, next, identity("r1"), context);
    let cold = build_cold(extracted("r1", files), identity("r1"));
    assert_eq!(relinked.stats, cold.stats, "{context}");
    assert_eq!(
        call(&relinked, "src/b.rs", "helper").1.to,
        A_HELPER,
        "{context}"
    );
}

#[test]
fn identity_change_relinks_cold() {
    let files = [A_RS, B_RS];
    let lexical = ViewIdentity {
        extractor_digest: extractor_digest(&[]),
        ..identity("r0")
    };
    let v0 = build_cold(extracted_with(&[], "r0", &files), lexical);
    assert!(
        v0.graph.files["src/b.rs"].edges.is_empty(),
        "lexical entries carry no edges"
    );
    assert_identity_change_is_cold(v0, &files, "other parser output");

    let without_java: Vec<BoxedExtractor> = registry()
        .into_iter()
        .filter(|extractor| extractor.dialect().id != "java")
        .collect();
    let with_java = [A_RS, B_RS, MAIN_JAVA];
    let old = ViewIdentity {
        extractor_digest: extractor_digest(&without_java),
        ..identity("r0")
    };
    let v0 = build_cold(extracted_with(&without_java, "r0", &with_java), old);
    let java = &v0.graph.files[MAIN_JAVA.0];
    assert!(
        matches!(java.coverage, FileCoverage::LexicalOnly { .. }),
        "{java:#?}"
    );
    assert_identity_change_is_cold(v0, &with_java, "java pack enabled");

    let bumped = ViewIdentity {
        resolver_version: RESOLVER_VERSION + 1,
        ..identity("r0")
    };
    let mut v0 = build_cold(extracted("r0", &files), bumped);
    let altered = call(&v0, "src/b.rs", "helper").0;
    let b_entry = v0.graph.files.get_mut("src/b.rs").unwrap();
    b_entry.edges[altered.index].to = func_id("src/b.rs", "run");
    assert_identity_change_is_cold(v0, &files, "resolver version bumped");
}

/// A 64-bit linear congruential generator with Knuth's MMIX constants: a
/// fixed seed replays one edit sequence with no new dependency.
pub(super) struct Lcg(pub(super) u64);

impl Lcg {
    pub(super) fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % bound
    }
}

pub(super) const NAMES: [&str; 4] = ["alpha", "beta", "gamma", "delta"];
/// File-name prefix and extension of each language in the mix.
pub(super) const LANGUAGES: [(&str, &str); 3] = [("r", "rs"), ("t", "ts"), ("p", "py")];

pub(super) fn seeded_path(language: usize, id: usize) -> String {
    let (prefix, extension) = LANGUAGES[language];
    format!("src/{prefix}{id}.{extension}")
}

/// The index into [`LANGUAGES`] of `path`'s extension, the first for another.
pub(super) fn language_of(path: &str) -> usize {
    let extension = path.rsplit('.').next().unwrap_or_default();
    LANGUAGES
        .iter()
        .position(|(_, known)| *known == extension)
        .unwrap_or(0)
}

/// A file defining one name and calling another, importing from a sibling
/// that may not exist: by name, by glob or whole module, or not at all.
pub(super) fn seeded_source(language: usize, rng: &mut Lcg) -> String {
    let (def, callee) = (NAMES[rng.below(4)], NAMES[rng.below(4)]);
    let module = format!("{}{}", LANGUAGES[language].0, rng.below(6));
    let (named, glob, body) = match LANGUAGES[language].1 {
        "rs" => (
            format!("use crate::{module}::{callee};\n"),
            format!("use crate::{module}::*;\n"),
            format!("pub fn {def}() {{\n    {callee}();\n}}\n"),
        ),
        "ts" => (
            format!("import {{ {callee} }} from \"./{module}\";\n"),
            format!("import * as {module} from \"./{module}\";\n"),
            format!("export function {def}() {{\n  {callee}();\n}}\n"),
        ),
        _ => (
            format!("from .{module} import {callee}\n"),
            format!("from .{module} import *\n"),
            format!("def {def}():\n    {callee}()\n"),
        ),
    };
    let header = match rng.below(3) {
        0 => String::new(),
        1 => named,
        _ => glob,
    };
    format!("{header}\n{body}")
}

/// Apply one add, remove, rename or edit to `files`; returns which.
pub(super) fn mutate(files: &mut BTreeMap<String, String>, rng: &mut Lcg) -> &'static str {
    let paths: Vec<String> = files.keys().cloned().collect();
    let victim = paths[rng.below(paths.len())].clone();
    let language = language_of(&victim);
    match rng.below(4) {
        0 => {
            let language = rng.below(LANGUAGES.len());
            let path = seeded_path(language, rng.below(8));
            files.insert(path, seeded_source(language, rng));
            "add"
        }
        1 if files.len() > 6 => {
            files.remove(&victim);
            "remove"
        }
        2 => {
            let body = files.remove(&victim).unwrap_or_default();
            files.insert(seeded_path(language, rng.below(8)), body);
            "rename"
        }
        _ => {
            files.insert(victim, seeded_source(language, rng));
            "edit"
        }
    }
}

pub(super) fn seeded_graph(revision: &str, files: &BTreeMap<String, String>) -> ResolvedGraph {
    let pairs: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    extracted(revision, &pairs)
}

#[test]
fn seeded_edit_sequence_relink_equals_cold() {
    let mut rng = Lcg(0x5EED_2026);
    let mut files: BTreeMap<String, String> = (0..12)
        .map(|n| (seeded_path(n % 3, n / 3), seeded_source(n % 3, &mut rng)))
        .collect();
    let mut view = build_cold(seeded_graph("s0", &files), identity("s0"));
    let mut ops = BTreeSet::new();
    let (mut ambiguous, mut bound) = (false, false);

    for step in 1..=50 {
        ops.insert(mutate(&mut files, &mut rng));
        let revision = format!("s{step}");
        let next = seeded_graph(&revision, &files);
        let context = format!("step {step}");
        view = assert_relink_equals_cold(&view, next, identity(&revision), &context);
        ambiguous |= view.stats.ambiguous > 0;
        bound |= view.stats.by_provenance.contains_key("unique-name");
    }

    assert_eq!(ops.len(), 4, "every kind of edit ran: {ops:?}");
    assert!(
        ambiguous && bound,
        "the sequence met candidate sets and unique binds"
    );
}
