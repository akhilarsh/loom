//! Persistence of resolved views.
//!
//! A base view lives at `<cache>/graph/view/<revision>-<identity digest>.json`,
//! shared by every worktree; a stage or local overlay view at
//! `<work>/context/<plan>/<stage>/view.json`. [`GraphStore::view`] serves a
//! view only when its identity equals the one requested: a file that fails to
//! parse or names another identity is rebuilt and rewritten, never served.
//!
//! A view is written only on a miss, so a warm `loom map` runs no resolution:
//! it parses one view file, and the base layer (with the overlay layer, when
//! there is one) that `ensure_snapshot` reads to check the layers are current.
//! A denied write keeps the view in memory for the rest of the process, as
//! layers do. A revision whose base layer does not load, missing or
//! unparseable, gets a view built in memory and never persisted, so the base
//! published or rebuilt later cannot be shadowed by an empty view.

use anyhow::{Context, Result};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::{build_cold, relink, ResolvedView, ViewIdentity};
use crate::context::graph_store::GraphStore;
use crate::context::store::canonical_json;

/// File name of an overlay view inside the overlay directory.
const OVERLAY_VIEW_FILE: &str = "view.json";

/// A stage overlay named by `(plan, stage)`.
type Overlay<'a> = Option<(&'a str, &'a str)>;

impl GraphStore {
    /// Where the view named by `identity` is persisted: a base view under
    /// `graph/view/`, an overlay view beside its overlay layer.
    pub fn view_path(&self, identity: &ViewIdentity, overlay: Overlay<'_>) -> PathBuf {
        match overlay {
            Some((plan, stage)) => self.overlay_dir(plan, stage).join(OVERLAY_VIEW_FILE),
            None => self.view_dir().join(format!(
                "{}-{}.json",
                identity.base_revision,
                identity.digest12()
            )),
        }
    }

    /// The resolved view of the base at `revision` with `overlay` applied.
    ///
    /// Returns, in order: the view this process just materialized, the
    /// persisted view of the requested identity, or a view relinked from the
    /// newest older base view (an overlay view: from its base view) and
    /// persisted. Only the last case resolves edges.
    pub fn view(&self, revision: &str, overlay: Overlay<'_>) -> Result<ResolvedView> {
        let (identity, overlay) = self.view_identity(revision, overlay)?;
        if let Some(view) = self.take_cached_view(&identity, overlay) {
            return Ok(view);
        }
        if let Some(view) = self.load_view(&identity, overlay) {
            return Ok(view);
        }
        self.build_view(identity, overlay)
    }

    /// Load the persisted view of `identity`, or `None` when there is none, it
    /// does not parse, or it names another identity. Never writes.
    pub(crate) fn load_view(
        &self,
        identity: &ViewIdentity,
        overlay: Overlay<'_>,
    ) -> Option<ResolvedView> {
        if !is_identifiable(identity, overlay) {
            return None;
        }
        let path = self.view_path(identity, overlay);
        let remembered = self.view_fallback.borrow().get(&path).cloned();
        let view = remembered.or_else(|| read_view(&path))?;
        if view.identity != *identity {
            tracing::warn!(path = %path.display(), "discarding a resolved view of another identity");
            return None;
        }
        Some(view)
    }

    /// Unless [`Self::has_view`] finds a current view of `revision` and
    /// `overlay`, build and persist it, and keep it for this process: the next
    /// [`Self::view`] call of its identity takes it instead of parsing the
    /// persisted file. A stale file is rebuilt without being parsed.
    pub(crate) fn materialize_view(&self, revision: &str, overlay: Overlay<'_>) -> Result<()> {
        let (identity, overlay) = self.view_identity(revision, overlay)?;
        if self.has_view(&identity, overlay) {
            return Ok(());
        }
        let path = self.view_path(&identity, overlay);
        let view = self.build_view(identity, overlay)?;
        self.view_cache.borrow_mut().insert(path, view);
        Ok(())
    }

    /// Whether a current view of `identity` is held in memory or persisted,
    /// told without parsing the view file. A base view file is current when it
    /// exists: its name carries the identity digest. An overlay view file has
    /// one name for every generation, so it is current only when it is not
    /// older than the overlay layer file. A view is written after the layer it
    /// describes; a layer rewritten since, by a reconcile that does not
    /// materialize, leaves it older. An overlay layer this process holds in
    /// memory has no file time to compare, so its view file is never current.
    fn has_view(&self, identity: &ViewIdentity, overlay: Overlay<'_>) -> bool {
        if !is_identifiable(identity, overlay) {
            return false;
        }
        let path = self.view_path(identity, overlay);
        let held = [&self.view_fallback, &self.view_cache].iter().any(|views| {
            views
                .borrow()
                .get(&path)
                .is_some_and(|view| view.identity == *identity)
        });
        if held {
            return true;
        }
        match overlay {
            None => path.is_file(),
            Some((plan, stage)) => {
                let layer = self.layer_modified(&self.overlay_path(plan, stage));
                let view = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
                matches!((view, layer), (Some(view), Some(layer)) if view >= layer)
            }
        }
    }

    /// Forget an overlay's view, on disk and in memory. Best-effort.
    pub(crate) fn discard_overlay_view(&self, plan: &str, stage: &str) {
        let path = self.overlay_dir(plan, stage).join(OVERLAY_VIEW_FILE);
        self.view_fallback.borrow_mut().remove(&path);
        self.view_cache.borrow_mut().remove(&path);
        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != ErrorKind::NotFound {
                tracing::debug!(path = %path.display(), %error, "failed to remove overlay view");
            }
        }
    }

    /// The identity a request resolves to, and the overlay it applies: one
    /// with no overlay layer is a base view.
    fn view_identity<'a>(
        &self,
        revision: &str,
        overlay: Overlay<'a>,
    ) -> Result<(ViewIdentity, Overlay<'a>)> {
        let layer = match overlay {
            Some((plan, stage)) => self.load_overlay(plan, stage)?,
            None => None,
        };
        Ok(match layer {
            Some(layer) => (ViewIdentity::current(revision, &layer.generation), overlay),
            None => (ViewIdentity::current(revision, ""), None),
        })
    }

    fn take_cached_view(
        &self,
        identity: &ViewIdentity,
        overlay: Overlay<'_>,
    ) -> Option<ResolvedView> {
        let path = self.view_path(identity, overlay);
        let view = self.view_cache.borrow_mut().remove(&path)?;
        (view.identity == *identity).then_some(view)
    }

    /// Resolve the view, relinking from a previous view when one is usable,
    /// and persist it when the base layer of its revision loaded: the view of
    /// a missing or unparseable base stays in memory.
    fn build_view(&self, identity: ViewIdentity, overlay: Overlay<'_>) -> Result<ResolvedView> {
        let revision = identity.base_revision.clone();
        let next = self.resolved(&revision, overlay)?;
        // `resolved` takes its revision from the layer `load_base` returned,
        // and leaves it empty for `None`.
        let base_loaded = next.base_revision == revision;
        let previous = match overlay {
            Some(_) => Some(self.view(&revision, None)?),
            None => self.newest_older_base_view(&identity),
        };
        let view = match previous {
            Some(previous) => relink(&previous, next, identity),
            None => build_cold(next, identity),
        };
        if base_loaded && is_identifiable(&view.identity, overlay) {
            self.persist_view(&view, overlay)?;
        }
        Ok(view)
    }

    /// The view of the newest other base that can seed a relink to `identity`.
    fn newest_older_base_view(&self, identity: &ViewIdentity) -> Option<ResolvedView> {
        self.older_view_files(&identity.base_revision)
            .into_iter()
            .find_map(|path| {
                read_view(&path).filter(|view| identity.relinkable_from(&view.identity))
            })
    }

    fn persist_view(&self, view: &ResolvedView, overlay: Overlay<'_>) -> Result<()> {
        let path = self.view_path(&view.identity, overlay);
        match write_view(&path, view) {
            Ok(()) => {
                if overlay.is_none() {
                    self.remove_other_identities(&view.identity.base_revision, &path);
                }
                Ok(())
            }
            Err(error) => self.fall_back_view_to_memory(&path, view, error),
        }
    }

    /// Delete the views of `revision` other than the one at `keep`: a bump of
    /// the resolver or extractors would otherwise leave them until the
    /// revision is pruned.
    fn remove_other_identities(&self, revision: &str, keep: &Path) {
        for path in self
            .view_files(revision)
            .into_iter()
            .filter(|path| path != keep)
        {
            if let Err(error) = fs::remove_file(&path) {
                tracing::debug!(path = %path.display(), %error, "failed to remove a superseded view");
            }
        }
    }
}

/// Whether an overlay view can be told apart from its predecessors. A layer
/// with no generation has no identity to key a persisted view by.
fn is_identifiable(identity: &ViewIdentity, overlay: Overlay<'_>) -> bool {
    overlay.is_none() || !identity.overlay_generation.is_empty()
}

/// Parse the view at `path`; a missing, unreadable or unparseable file is a
/// miss.
fn read_view(path: &Path) -> Option<ResolvedView> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cannot read a resolved view");
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(view) => Some(view),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "discarding an unparseable resolved view");
            None
        }
    }
}

/// Write `view` with a locked, crash-atomic replacement.
fn write_view(path: &Path, view: &ResolvedView) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create view directory: {}", parent.display()))?;
    }
    let content = canonical_json(view)?;
    crate::fs::locking::locked_write(path, &content)
        .with_context(|| format!("Failed to write resolved view: {}", path.display()))
}
