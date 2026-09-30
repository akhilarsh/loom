//! In-memory fallback for a denied cache write.
//!
//! `.loom/cache/context-v1` under a checkout- or worktree-rooted session may
//! be read-only — the phase-3 confinement deny, or a worktree's narrower
//! sandbox grants (`doc/plans/PLAN-loom-state-confinement.md` §14 item 1).
//! `loom map` and `loom knowledge context` must still answer from the layer
//! they just built rather than failing or silently reading back nothing:
//! `GraphStore::fall_back_to_memory` keeps that layer in
//! `GraphStore::memory_fallback` for the rest of this process, and
//! `GraphStore::read_layer_or_memory` prefers it over disk. Neither the
//! fallback nor its contents ever reach disk; the next process starts over
//! and, once write access is restored, persists normally again.

use anyhow::Result;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use super::{read_layer, GraphLayer, GraphStore};
use crate::context::view::ResolvedView;

impl GraphStore {
    /// A denied cache write is not this call's failure: keep `layer` in
    /// memory for the rest of this process's reads instead of failing the
    /// caller. A genuine bug — a malformed path, a serialization failure —
    /// still propagates.
    pub(super) fn fall_back_to_memory(
        &self,
        path: &Path,
        layer: &GraphLayer,
        error: anyhow::Error,
    ) -> Result<()> {
        if !is_write_denied(&error) {
            return Err(error);
        }
        tracing::warn!(
            path = %path.display(),
            %error,
            "context cache is not writable; keeping this source graph layer in memory \
             for the rest of this process"
        );
        self.memory_fallback
            .borrow_mut()
            .insert(path.to_path_buf(), layer.clone());
        Ok(())
    }

    /// [`Self::fall_back_to_memory`] for a resolved view: keep `view` in memory
    /// when the disk write of `error` was denied, propagate any other failure.
    pub(crate) fn fall_back_view_to_memory(
        &self,
        path: &Path,
        view: &ResolvedView,
        error: anyhow::Error,
    ) -> Result<()> {
        if !is_write_denied(&error) {
            return Err(error);
        }
        tracing::warn!(
            path = %path.display(),
            %error,
            "context cache is not writable; keeping this resolved view in memory \
             for the rest of this process"
        );
        self.view_fallback
            .borrow_mut()
            .insert(path.to_path_buf(), view.clone());
        Ok(())
    }

    /// True when a denied cache write made this store keep a layer in memory
    /// only, so that layer will not outlive the process.
    pub fn fell_back(&self) -> bool {
        !self.memory_fallback.borrow().is_empty() || !self.view_fallback.borrow().is_empty()
    }

    /// When the layer file at `path` was last modified, or `None` when there is
    /// none or this process holds the layer in memory: a denied write left any
    /// file there older than the layer this process reads.
    pub(crate) fn layer_modified(&self, path: &Path) -> Option<SystemTime> {
        if self.memory_fallback.borrow().contains_key(path) {
            return None;
        }
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    }

    /// `read_layer`, preferring a layer this process already fell back to
    /// in memory for this exact path.
    pub(super) fn read_layer_or_memory(&self, path: &Path) -> Result<Option<GraphLayer>> {
        if let Some(layer) = self.memory_fallback.borrow().get(path) {
            return Ok(Some(layer.clone()));
        }
        read_layer(path)
    }
}

/// True when `error` (or something in its cause chain) is a filesystem
/// permission failure rather than a genuine bug — mirrors
/// `commands/memory/handlers/record.rs::is_write_denied` and
/// `telemetry/spool.rs::is_write_denied`, which cannot be reused here
/// directly (module-private to unrelated subsystems).
fn is_write_denied(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_err| {
                io_err.kind() == std::io::ErrorKind::PermissionDenied
                    || io_err.raw_os_error() == Some(libc::EROFS)
            })
    })
}

#[cfg(test)]
#[path = "fallback_tests.rs"]
mod tests;
