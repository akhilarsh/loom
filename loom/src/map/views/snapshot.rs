//! Which graph a `loom map` answer came from: the snapshot identity every
//! JSON payload and source window carries, and the loader that reads it.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::{json, Value};

use crate::context::extract::registry;
use crate::context::freshness::GraphState;
use crate::context::graph_store::{GraphStore, ResolvedGraph};
use crate::context::refresh::{clean_generation, short_revision, SnapshotOutcome};
use crate::context::source_graph::{FileCoverage, GRAPH_SCHEMA_VERSION};

/// The freshness state and layer identity of the graph a view was answered from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotIdentity {
    pub state: GraphState,
    /// The base revision served; after a failed build, the stale base.
    pub base_revision: String,
    pub overlay: Option<(String, String)>,
    pub generation: String,
    pub built_at: Option<String>,
    pub persisted: bool,
    pub schema_version: u32,
    /// Dialect id to the parser version its extractor stamps.
    pub extractors: BTreeMap<String, String>,
}

impl SnapshotIdentity {
    /// `<state> <base rev8>[+<generation8>]`, the identity column of a
    /// source-window header. The generation is shown only when it names more
    /// than the base commit (an overlay or a dirty tree).
    pub fn window_label(&self) -> String {
        let base = short_revision(&self.base_revision);
        let names_more = self.overlay.is_some()
            || (!self.generation.is_empty()
                && self.generation != clean_generation(&self.base_revision));
        if names_more {
            format!(
                "{} {base}+{}",
                self.state.as_str(),
                short_revision(&self.generation)
            )
        } else {
            format!("{} {base}", self.state.as_str())
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "state": self.state.as_str(),
            "base_revision": self.base_revision,
            "overlay": self.overlay.as_ref().map(|(plan, stage)| json!({"plan": plan, "stage": stage})),
            "generation": self.generation,
            "built_at": self.built_at,
            "persisted": self.persisted,
            "schema_version": self.schema_version,
            "extractors": self.extractors,
        })
    }
}

/// Read the base and overlay layers `snapshot` selected, once each, and merge
/// them into the graph views query. The identity's generation and build time
/// come from the same loaded layers, so nothing is parsed twice.
///
/// A missing base is not an error: the graph is empty and the identity says
/// why through its state.
pub fn load_graph(
    graph_store: &GraphStore,
    snapshot: &SnapshotOutcome,
) -> Result<(ResolvedGraph, SnapshotIdentity)> {
    let base = graph_store
        .load_base(&snapshot.revision)?
        .unwrap_or_default();
    let overlay = match &snapshot.overlay {
        Some((plan, stage)) => graph_store.load_overlay(plan, stage)?,
        None => None,
    };
    let (generation, built_at) = match &overlay {
        Some(layer) => (layer.generation.clone(), layer.built_at),
        None => (base.generation.clone(), base.built_at),
    };
    let identity = SnapshotIdentity {
        state: snapshot.state(),
        base_revision: snapshot.revision.clone(),
        overlay: snapshot.overlay.clone(),
        generation: if generation.is_empty() {
            snapshot.generation.clone()
        } else {
            generation
        },
        built_at: built_at.map(|at| at.to_rfc3339()),
        persisted: snapshot.persisted,
        schema_version: GRAPH_SCHEMA_VERSION,
        extractors: extractor_versions(),
    };

    let mut graph = ResolvedGraph {
        base_revision: base.revision,
        overlaid: BTreeSet::new(),
        files: base.files,
    };
    for (path, entry) in overlay.into_iter().flat_map(|layer| layer.files) {
        // An overlay entry is the complete truth for its file in this stage.
        if entry.coverage == FileCoverage::Deleted {
            graph.files.remove(&path);
        } else {
            graph.overlaid.insert(path.clone());
            graph.files.insert(path, entry);
        }
    }
    Ok((graph, identity))
}

fn extractor_versions() -> BTreeMap<String, String> {
    registry()
        .iter()
        .map(|extractor| {
            (
                extractor.dialect().id.to_string(),
                extractor.cache_identity().to_parser_version(),
            )
        })
        .collect()
}
