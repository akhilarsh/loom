//! Relink equals the cold build across transitions the seeded edit mix of
//! `tests_equivalence` never makes: a file turning unreadable (a symlink the
//! refresh path refuses) or readable again, and a re-export chain changing.

use std::collections::{BTreeMap, BTreeSet};

use super::tests::{assert_relink_equals_cold, extracted, identity};
use super::tests_equivalence::{
    language_of, mutate, seeded_graph, seeded_path, seeded_source, unreadable, Lcg, LANGUAGES,
    NAMES,
};
use super::*;
use crate::context::source_graph::file_node_id;

const APP_TS: (&str, &str) = (
    "src/app.ts",
    "import \"./util\";\n\nexport function main() {}\n",
);
const UTIL_TS: (&str, &str) = ("src/util.ts", "export function parse() {}\n");

/// `app.ts` and `util.ts` at `revision`, with `util.ts` unreadable.
fn util_unreadable(revision: &str) -> ResolvedGraph {
    let mut graph = extracted(revision, &[APP_TS]);
    graph
        .files
        .insert(UTIL_TS.0.to_string(), unreadable("symlink refused"));
    graph
}

#[test]
fn a_file_gaining_or_losing_its_file_node_reopens_module_path_lookups() {
    let util = file_node_id(std::path::Path::new(UTIL_TS.0));
    let v0 = build_cold(util_unreadable("r0"), identity("r0"));
    let next = extracted("r1", &[APP_TS, UTIL_TS]);

    let v1 = assert_relink_equals_cold(&v0, next, identity("r1"), "symlink became a file");

    assert!(
        v1.graph.edges().any(|edge| edge.to == util),
        "the import of ./util never bound, so the transition went unexercised: {:#?}",
        v1.graph.files[APP_TS.0].edges
    );
    assert_relink_equals_cold(
        &v1,
        util_unreadable("r2"),
        identity("r2"),
        "file became a symlink",
    );
}

/// A file that only re-exports one name from a sibling module that may not
/// exist, in the language of `path`.
fn reexport_source(path: &str, rng: &mut Lcg) -> String {
    let language = language_of(path);
    let name = NAMES[rng.below(NAMES.len())];
    let module = format!("{}{}", LANGUAGES[language].0, rng.below(6));
    match LANGUAGES[language].1 {
        "rs" => format!("pub use crate::{module}::{name};\n"),
        "ts" => format!("export {{ {name} }} from \"./{module}\";\n"),
        _ => format!("from .{module} import {name}\n"),
    }
}

/// Apply one seeded step: an edit of [`mutate`], a file turning unreadable or
/// readable again, or a file rewritten as a re-export; returns which.
fn step(
    files: &mut BTreeMap<String, String>,
    unreadable_paths: &mut BTreeSet<String>,
    rng: &mut Lcg,
) -> &'static str {
    let paths: Vec<String> = files.keys().cloned().collect();
    let victim = paths[rng.below(paths.len())].clone();
    match rng.below(3) {
        0 => mutate(files, rng),
        1 if unreadable_paths.remove(&victim) => "readable",
        1 => {
            unreadable_paths.insert(victim);
            "unreadable"
        }
        _ => {
            let source = reexport_source(&victim, rng);
            files.insert(victim, source);
            "re-export"
        }
    }
}

/// [`seeded_graph`] with every present path of `unreadable_paths` unreadable.
fn graph_with(
    revision: &str,
    files: &BTreeMap<String, String>,
    unreadable_paths: &BTreeSet<String>,
) -> ResolvedGraph {
    let mut graph = seeded_graph(revision, files);
    for path in unreadable_paths {
        if let Some(entry) = graph.files.get_mut(path) {
            *entry = unreadable("symlink refused");
        }
    }
    graph
}

#[test]
fn seeded_transitions_relink_equals_cold() {
    let mut rng = Lcg(0x5EED_2027);
    let mut files: BTreeMap<String, String> = (0..12)
        .map(|n| (seeded_path(n % 3, n / 3), seeded_source(n % 3, &mut rng)))
        .collect();
    let mut unreadable_paths = BTreeSet::new();
    let mut view = build_cold(graph_with("t0", &files, &unreadable_paths), identity("t0"));
    let mut ops = BTreeSet::new();

    for n in 1..=60 {
        ops.insert(step(&mut files, &mut unreadable_paths, &mut rng));
        let revision = format!("t{n}");
        let next = graph_with(&revision, &files, &unreadable_paths);
        view = assert_relink_equals_cold(&view, next, identity(&revision), &format!("step {n}"));
    }

    for op in ["unreadable", "readable", "re-export"] {
        assert!(ops.contains(op), "{op} never ran: {ops:?}");
    }
}
