//! View identity, the dependency index, the origin marker, the run counter
//! and scripted relink changes, plus the helpers the equivalence tests share.

use std::collections::BTreeMap;
use std::path::Path;

use super::build::resolution_runs;
use super::identity::{digest_versions, extractor_digest};
use super::*;
use crate::context::extract::lexical::LEXICAL_PARSER_VERSION;
use crate::context::extract::{extract_file, registry, BoxedExtractor};
use crate::context::graph_store::FileEntry;
use crate::context::resolve::EdgeRef;
use crate::context::source_graph::{
    body_hash, SourceEdge, SourceEdgeKind, GRAPH_SCHEMA_VERSION, UNRESOLVED_TARGET,
};

pub(super) const A_RS: (&str, &str) = ("src/a.rs", "pub fn helper() {}\n");
pub(super) const B_RS: (&str, &str) = ("src/b.rs", "pub fn run() {\n    helper();\n}\n");
pub(super) const C_RS: (&str, &str) = ("src/c.rs", "pub fn helper() {}\n");
pub(super) const A_HELPER: &str = "src/a.rs#function:helper";

/// Extract every `(path, source)` pair through `extractors` into a graph of
/// extraction-time edges.
pub(super) fn extracted_with(
    extractors: &[BoxedExtractor],
    revision: &str,
    files: &[(&str, &str)],
) -> ResolvedGraph {
    let files: BTreeMap<String, FileEntry> = files
        .iter()
        .map(|(path, source)| {
            let extraction = extract_file(extractors, Path::new(path), source.as_bytes());
            let entry = FileEntry::from_extraction(source.as_bytes(), extraction);
            (path.to_string(), entry)
        })
        .collect();
    ResolvedGraph {
        base_revision: revision.to_string(),
        files,
        ..ResolvedGraph::default()
    }
}

/// [`extracted_with`] through every registered extractor.
pub(super) fn extracted(revision: &str, files: &[(&str, &str)]) -> ResolvedGraph {
    extracted_with(&registry(), revision, files)
}

pub(super) fn identity(revision: &str) -> ViewIdentity {
    ViewIdentity::current(revision, "")
}

/// Relink `previous` onto `next`, assert it equals the cold build of `next`
/// byte for byte, and return the relinked view.
pub(super) fn assert_relink_equals_cold(
    previous: &ResolvedView,
    next: ResolvedGraph,
    identity: ViewIdentity,
    context: &str,
) -> ResolvedView {
    let relinked = relink(previous, next.clone(), identity.clone());
    let cold = build_cold(next, identity);
    let relinked_json = String::from_utf8(canonical_bytes(&relinked).unwrap()).unwrap();
    let cold_json = String::from_utf8(canonical_bytes(&cold).unwrap()).unwrap();
    assert!(
        relinked_json == cold_json,
        "{context}: relink differs from the cold build {}",
        first_difference(&relinked_json, &cold_json)
    );
    relinked
}

/// The first line two JSON texts differ on, with a few lines around it.
fn first_difference(relinked: &str, cold: &str) -> String {
    let shorter = relinked.lines().count().min(cold.lines().count());
    let line = relinked
        .lines()
        .zip(cold.lines())
        .position(|(left, right)| left != right)
        .unwrap_or(shorter);
    let window = |text: &str| -> String {
        let start = line.saturating_sub(6);
        text.lines()
            .skip(start)
            .take(10)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (left, right) = (window(relinked), window(cold));
    format!("at line {line}\nrelink:\n{left}\ncold:\n{right}")
}

/// The first call to `symbol` extracted from `caller`, with its reference.
pub(super) fn call<'v>(
    view: &'v ResolvedView,
    caller: &str,
    symbol: &str,
) -> (EdgeRef, &'v SourceEdge) {
    let entry = &view.graph.files[caller];
    let (index, edge) = entry
        .edges
        .iter()
        .enumerate()
        .find(|(_, edge)| edge.kind == SourceEdgeKind::Calls && edge.symbol == symbol)
        .unwrap_or_else(|| panic!("no call to {symbol} in {caller}: {:#?}", entry.edges));
    let path = caller.to_string();
    (EdgeRef { path, index }, edge)
}

#[test]
fn identity_digest_is_stable_and_tracks_resolver_version() {
    let id = identity("r0");
    let digest = id.digest12();

    assert_eq!(digest.len(), 12);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()), "{digest}");
    assert_eq!(identity("r0").digest12(), digest, "stable across calls");
    let bumped = ViewIdentity {
        resolver_version: RESOLVER_VERSION + 1,
        ..id.clone()
    };
    assert_ne!(
        bumped.digest12(),
        digest,
        "a resolver bump renames the view"
    );
    assert_ne!(identity("r1").digest12(), digest);
}

#[test]
fn current_identity_names_schema_extractors_and_resolver() {
    let id = ViewIdentity::current("r0", "g1");

    assert_eq!(id.schema_version, GRAPH_SCHEMA_VERSION);
    assert_eq!(id.resolver_version, RESOLVER_VERSION);
    assert_eq!(
        (id.base_revision.as_str(), id.overlay_generation.as_str()),
        ("r0", "g1")
    );
    assert_eq!(id.extractor_digest, extractor_digest(&registry()));
    assert_ne!(
        id.extractor_digest,
        extractor_digest(&[]),
        "it names the extractors"
    );
}

#[test]
fn extractor_digest_names_the_lexical_parser_version() {
    let lexical_line = format!("lexical={LEXICAL_PARSER_VERSION}\n");
    assert_eq!(
        extractor_digest(&[]),
        body_hash(lexical_line.as_bytes()),
        "the lexical fallback's version is a digest line"
    );

    let extractors = registry();
    assert_eq!(
        digest_versions(&extractors, LEXICAL_PARSER_VERSION),
        extractor_digest(&extractors)
    );
    assert_ne!(
        digest_versions(&extractors, "lexical+bumped"),
        extractor_digest(&extractors),
        "a lexical bump renames every view"
    );
}

#[test]
fn dependency_index_lists_the_edges_that_consulted_a_key() {
    let view = build_cold(extracted("r0", &[A_RS, B_RS, C_RS]), identity("r0"));
    let (b_call, edge) = call(&view, "src/b.rs", "helper");

    assert_eq!(edge.candidates, [A_HELPER, "src/c.rs#function:helper"]);
    let consulted: Vec<&EdgeRef> = view.deps.edges("name:rust:helper").collect();
    assert_eq!(
        consulted,
        [&b_call],
        "the candidate set is indexed under its name"
    );
    assert_eq!(view.deps.edges("name:rust:absent").count(), 0);
}

#[test]
fn removing_a_caller_drops_the_keys_only_it_consulted() {
    let v0 = build_cold(extracted("r0", &[A_RS, B_RS]), identity("r0"));
    assert_eq!(v0.deps.edges("name:rust:helper").count(), 1);

    let next = extracted("r1", &[A_RS]);
    let relinked = assert_relink_equals_cold(&v0, next, identity("r1"), "caller removed");

    assert_eq!(relinked.deps.edges("name:rust:helper").count(), 0);
}

#[test]
fn a_binding_into_a_removed_file_is_reselected_without_a_key_match() {
    let mut v0 = build_cold(extracted("r0", &[A_RS, B_RS]), identity("r0"));
    assert_eq!(call(&v0, "src/b.rs", "helper").1.to, A_HELPER);
    // No recorded key: only the rule for edges into a removed file can
    // reopen the call.
    v0.deps = DependencyIndex::default();

    let next = extracted("r1", &[B_RS]);
    let relinked = assert_relink_equals_cold(&v0, next, identity("r1"), "target removed");

    assert!(call(&relinked, "src/b.rs", "helper").1.is_unresolved());
}

#[test]
fn build_cold_and_relink_each_count_one_resolution_run() {
    let before = resolution_runs();
    let v0 = build_cold(extracted("r0", &[A_RS, B_RS]), identity("r0"));
    relink(&v0, extracted("r1", &[A_RS, B_RS, C_RS]), identity("r1"));
    let stale = ViewIdentity {
        resolver_version: RESOLVER_VERSION + 1,
        ..identity("r2")
    };
    relink(&v0, extracted("r2", &[A_RS]), stale);

    assert_eq!(
        resolution_runs() - before,
        3,
        "a fallback relink runs cold once"
    );
}

#[test]
fn origin_is_built_in_process_and_never_serialized() {
    let built = build_cold(extracted("r0", &[A_RS, B_RS]), identity("r0"));
    assert_eq!(built.origin, ViewOrigin::Built);
    assert_eq!(built.origin.as_str(), "built");

    let bytes = canonical_bytes(&built).unwrap();
    let parsed: ResolvedView = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(parsed.origin, ViewOrigin::Materialized);
    assert_eq!(canonical_bytes(&parsed).unwrap(), bytes);
    let rebuilt = ResolvedView {
        origin: ViewOrigin::Built,
        ..parsed
    };
    assert_eq!(rebuilt, built);
}

type Files = &'static [(&'static str, &'static str)];

/// One scripted change: the files before and after, and where the call to
/// `symbol` in `caller` points in the cold build of each.
struct Change {
    name: &'static str,
    before: Files,
    after: Files,
    caller: &'static str,
    symbol: &'static str,
    targets: (&'static str, &'static str),
}

const A_EDITED: (&str, &str) = ("src/a.rs", "pub fn helper() {\n    let _unused = 1;\n}\n");
const D_RS: (&str, &str) = ("src/d.rs", "pub fn helper() {}\n");
const B_GLOB: (&str, &str) = (
    "src/b.rs",
    "use external::*;\n\npub fn run() {\n    helper();\n}\n",
);
const X_TS: (&str, &str) = ("src/x.ts", "export function parse() {}\n");
const Y_TS: (&str, &str) = ("src/y.ts", "export function parse() {}\n");
const MAIN_X: (&str, &str) = (
    "src/main.ts",
    "import { parse } from \"./x\";\n\nexport function run() {\n  parse();\n}\n",
);
const MAIN_Y: (&str, &str) = (
    "src/main.ts",
    "import { parse } from \"./y\";\n\nexport function run() {\n  parse();\n}\n",
);
const GO_MAIN: (&str, &str) = (
    "src/app/main.go",
    "package app\n\nfunc Run() {\n\thelper()\n}\n",
);
const GO_ONE: (&str, &str) = ("src/one/one.go", "package one\n\nfunc helper() {}\n");
const GO_TWO: (&str, &str) = ("src/two/two.go", "package two\n\nfunc helper() {}\n");
const GO_UTIL: (&str, &str) = ("src/app/util.go", "package app\n\nfunc helper() {}\n");

/// The two-file Rust graph most changes start from: `run` calls `helper`.
const A_AND_B: Files = &[A_RS, B_RS];

const fn rust_change(name: &'static str, after: Files, to: &'static str) -> Change {
    Change {
        name,
        before: A_AND_B,
        after,
        caller: "src/b.rs",
        symbol: "helper",
        targets: (A_HELPER, to),
    }
}

const CHANGES: &[Change] = &[
    rust_change("namesake added", &[A_RS, B_RS, C_RS], UNRESOLVED_TARGET),
    rust_change("bound target's file deleted", &[B_RS], UNRESOLVED_TARGET),
    rust_change("file renamed", &[B_RS, D_RS], "src/d.rs#function:helper"),
    rust_change(
        "glob refuses a unique name",
        &[A_RS, B_GLOB],
        UNRESOLVED_TARGET,
    ),
    rust_change("edit keeps the definitions", &[A_EDITED, B_RS], A_HELPER),
    Change {
        name: "import path changed",
        before: &[X_TS, Y_TS, MAIN_X],
        after: &[X_TS, Y_TS, MAIN_Y],
        caller: "src/main.ts",
        symbol: "parse",
        targets: ("src/x.ts#function:parse", "src/y.ts#function:parse"),
    },
    Change {
        name: "same-package definition turns a candidate set into a bind",
        before: &[GO_MAIN, GO_ONE, GO_TWO],
        after: &[GO_MAIN, GO_ONE, GO_TWO, GO_UTIL],
        caller: "src/app/main.go",
        symbol: "helper",
        targets: (UNRESOLVED_TARGET, "src/app/util.go#function:helper"),
    },
];

#[test]
fn scripted_changes_relink_equal_cold() {
    for change in CHANGES {
        let v0 = build_cold(extracted("r0", change.before), identity("r0"));
        let next = extracted("r1", change.after);
        let v1 = assert_relink_equals_cold(&v0, next, identity("r1"), change.name);
        let before = call(&v0, change.caller, change.symbol).1.to.as_str();
        let after = call(&v1, change.caller, change.symbol).1.to.as_str();
        assert_eq!((before, after), change.targets, "{}", change.name);
    }
}
