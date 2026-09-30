//! Labelled ids the corpus graph has no node for. A mistyped id can only score
//! as a miss, which a threshold below 1.0 absorbs, so it is reported apart from
//! the metrics as a defect of the labels.

use std::collections::BTreeSet;

use super::labels::{Expectation, Labels};
use crate::context::graph_store::ResolvedGraph;

/// One message per labelled declaration path, target, ambiguous candidate and
/// impact id that names nothing in `graph`. An external label names no node and
/// is exempt.
///
/// A declaration carries no id: it is matched on `(path, kind, scope)`, and a
/// node missing for a real file is a `declaration_recall` miss, so only its
/// path is checked here.
pub(super) fn unknown_label_ids(labels: &Labels, graph: &ResolvedGraph) -> Vec<String> {
    let known: BTreeSet<&str> = graph.nodes().map(|node| node.id.as_str()).collect();
    let mut unknown = Vec::new();
    declaration_paths(labels, graph, &mut unknown);
    reference_ids(labels, &known, &mut unknown);
    impact_ids(labels, &known, &mut unknown);
    unknown
}

fn declaration_paths(labels: &Labels, graph: &ResolvedGraph, unknown: &mut Vec<String>) {
    for label in &labels.declarations {
        if !graph.files.contains_key(&label.path) {
            unknown.push(format!(
                "{}:{} declaration {} {}: path {} names no file of the corpus graph",
                label.path,
                label.line,
                label.kind,
                label.scope.join("::"),
                label.path
            ));
        }
    }
}

fn reference_ids(labels: &Labels, known: &BTreeSet<&str>, unknown: &mut Vec<String>) {
    for label in &labels.references {
        let (what, ids): (&str, Vec<&str>) = match label.expect.expectation() {
            Expectation::Target(id) => ("target", vec![id]),
            Expectation::Ambiguous(ids) => (
                "ambiguous candidate",
                ids.iter().map(String::as_str).collect(),
            ),
            Expectation::External => continue,
        };
        for id in ids.into_iter().filter(|id| !known.contains(id)) {
            unknown.push(format!(
                "{}:{} {}: {what} {id} names no node of the corpus graph",
                label.path, label.line, label.symbol
            ));
        }
    }
}

fn impact_ids(labels: &Labels, known: &BTreeSet<&str>, unknown: &mut Vec<String>) {
    for label in &labels.impact {
        let ids = std::iter::once(("start", label.start.as_str()))
            .chain(label.expect.iter().map(|id| ("expected", id.as_str())));
        for (role, id) in ids.filter(|(_, id)| !known.contains(id)) {
            unknown.push(format!(
                "impact {} depth {}: {role} id {id} names no node of the corpus graph",
                label.start, label.depth
            ));
        }
    }
}
