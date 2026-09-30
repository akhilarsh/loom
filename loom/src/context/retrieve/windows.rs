//! Source windows on a retrieved pack: the exact lines of the strongest
//! matches, read back from the tree the graph was built from.
//!
//! Attached after selection, so a window never changes which items are chosen;
//! it is kept only while the whole pack still fits its budget.

use super::Surface;
use crate::context::freshness::GraphState;
use crate::context::graph_store::ResolvedGraph;
use crate::context::render::rendered_item_tokens;
use crate::context::schema::{ContextItem, ContextPack, ItemKind, SelectionReason};
use crate::context::source_graph::FileCoverage;
use crate::context::window::read_window;
use std::path::Path;

/// The most windows one pack carries.
const MAX_WINDOWS: usize = 2;

/// The most lines one window carries.
const WINDOW_LINES: usize = 12;

/// Attach up to [`MAX_WINDOWS`] windows to `pack`.
///
/// Nothing is attached for [`Surface::Hook`], whose output stays compact, nor
/// to a pack whose semantic layer is not current: a window quotes the file on
/// disk, and only a current graph vouches for the lines it names. An item
/// qualifies when it matched by id, exact symbol or exact path and its file
/// was extracted with full coverage. A window is dropped again when charging
/// it would push the pack over its budget.
pub(crate) fn attach_windows(
    pack: &mut ContextPack,
    graph: &ResolvedGraph,
    project_root: &Path,
    surface: Surface,
) {
    if surface == Surface::Hook || pack.semantic_freshness.state() != GraphState::Current {
        return;
    }
    let mut attached = 0;
    for index in 0..pack.items.len() {
        if attached == MAX_WINDOWS {
            break;
        }
        if !qualifies(&pack.items[index], graph) {
            continue;
        }
        let id = pack.items[index].id.as_str().to_string();
        let Ok(window) = read_window(graph, project_root, &id, WINDOW_LINES) else {
            continue;
        };
        if charge_window(pack, index, window.text) {
            attached += 1;
        }
    }
}

/// A source node matched strongly by name, whose file the graph read in full.
fn qualifies(item: &ContextItem, graph: &ResolvedGraph) -> bool {
    item.kind == ItemKind::SourceNode
        && item.reasons.iter().any(|reason| {
            matches!(
                reason,
                SelectionReason::ExplicitId
                    | SelectionReason::ExactSymbol
                    | SelectionReason::ExactPath
            )
        })
        && item
            .pointer
            .path
            .to_str()
            .and_then(|path| graph.files.get(path))
            .is_some_and(|entry| entry.coverage == FileCoverage::Full)
}

/// Store `text` as the window of `pack.items[index]` and charge it: to the
/// item, to the pack's estimate and to the coverage's `included_tokens`.
/// Returns `false`, with the pack as it was, when the charged pack would exceed
/// its budget.
fn charge_window(pack: &mut ContextPack, index: usize, text: String) -> bool {
    let before = pack.items[index].token_count;
    pack.items[index].window = Some(text);
    let after = rendered_item_tokens(&pack.items[index]);
    let cost = after.saturating_sub(before);
    pack.items[index].token_count = after;
    pack.omitted.coverage.included_tokens += cost;
    pack.recompute_estimate();
    if pack.within_budget() {
        return true;
    }
    pack.items[index].window = None;
    pack.items[index].token_count = before;
    pack.omitted.coverage.included_tokens -= cost;
    pack.recompute_estimate();
    false
}
