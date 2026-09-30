//! Which graph a `loom map` answer came from: the snapshot identity every
//! JSON payload and source window carries, and the loader that reads it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::context::extract::registry;
use crate::context::freshness::GraphState;
use crate::context::graph_store::GraphStore;
use crate::context::refresh::{clean_generation, short_revision, SnapshotOutcome};
use crate::context::source_graph::GRAPH_SCHEMA_VERSION;
use crate::context::view::{ResolvedView, ViewIdentity, ViewOrigin};

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
    /// Version of the cross-file resolution rules the view was resolved under.
    pub resolver_version: u32,
    /// Whether the view was read from disk or resolved by this process.
    pub view: ViewOrigin,
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
            "resolver_version": self.resolver_version,
            "view": self.view.as_str(),
            "extractors": self.extractors,
        })
    }
}

/// Describe where `view`, the resolved view `snapshot` selected, came from.
/// The generation comes from the view's identity, so no layer is parsed.
///
/// A missing base is not an error: the view is empty and the identity says
/// why through its state.
pub fn identity_of(
    graph_store: &GraphStore,
    snapshot: &SnapshotOutcome,
    view: &ResolvedView,
) -> SnapshotIdentity {
    let generation = &view.identity.overlay_generation;
    let layer_path = match &snapshot.overlay {
        Some((plan, stage)) => graph_store.overlay_path(plan, stage),
        None => graph_store.base_path(&snapshot.revision),
    };
    SnapshotIdentity {
        state: snapshot.state(),
        base_revision: snapshot.revision.clone(),
        overlay: snapshot.overlay.clone(),
        generation: if generation.is_empty() {
            snapshot.generation.clone()
        } else {
            generation.clone()
        },
        built_at: layer_written_at(&layer_path),
        persisted: snapshot.persisted,
        schema_version: GRAPH_SCHEMA_VERSION,
        resolver_version: view.identity.resolver_version,
        view: view.origin,
        extractors: ViewIdentity::extractor_versions(&registry()),
    }
}

/// When the layer file at `path` was last written, as RFC 3339; `None` for a
/// layer that exists only in memory. Read from the file's metadata so the
/// layer itself is never parsed.
fn layer_written_at(path: &Path) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Utc>::from(modified).to_rfc3339())
}
