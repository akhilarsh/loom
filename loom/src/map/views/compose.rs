//! Turns one `loom map` request into its views, in human or JSON form.

use std::path::Path;

use colored::Colorize;
use serde_json::{json, Map, Value};

use crate::context::graph_store::ResolvedGraph;
use crate::context::window::SourceWindow;
use crate::context::ResolutionStats;

use super::json::{footer_json, MAP_SCHEMA};
use super::snapshot::SnapshotIdentity;
use super::{
    callees_json, callers_json, find_all_json, impact_json, outline_json, references_json,
    render_callees, render_callers, render_find_all, render_footer, render_impact, render_outline,
    render_references, render_window, window_json, ViewOptions,
};

/// Every view a request names. The window is read by the caller, whose exit
/// codes depend on the outcome.
#[derive(Debug)]
pub struct ViewQuery {
    pub outline: Option<String>,
    pub find_all: Option<String>,
    pub impact: Option<String>,
    pub callers: Option<String>,
    pub callees: Option<String>,
    pub references: Option<String>,
    pub window: Option<SourceWindow>,
    pub options: ViewOptions,
}

/// The graph a request is answered from.
pub struct ViewContext<'a> {
    pub graph: &'a ResolvedGraph,
    pub project_root: &'a Path,
    pub stats: &'a ResolutionStats,
    pub snapshot: &'a SnapshotIdentity,
}

/// The requested views as `(name, rendered text)`, in a fixed order.
pub fn human_views(ctx: &ViewContext<'_>, query: &ViewQuery) -> Vec<(&'static str, String)> {
    let (graph, root, opts) = (ctx.graph, ctx.project_root, &query.options);
    let mut views = Vec::new();
    if let Some(arg) = &query.outline {
        views.push(("outline", render_outline(graph, root, arg)));
    }
    if let Some(arg) = &query.find_all {
        views.push(("find-all", render_find_all(graph, arg, opts)));
    }
    if let Some(arg) = &query.impact {
        views.push(("impact", render_impact(graph, root, arg, ctx.stats, opts)));
    }
    if let Some(arg) = &query.callers {
        views.push(("callers", render_callers(graph, root, arg, opts)));
    }
    if let Some(arg) = &query.callees {
        views.push(("callees", render_callees(graph, root, arg, opts)));
    }
    if let Some(arg) = &query.references {
        views.push(("references", render_references(graph, root, arg, opts)));
    }
    if let Some(window) = &query.window {
        views.push(("window", render_window(window, ctx.snapshot)));
    }
    views
}

/// Headings when more than one view is shown, then the coverage footer. A
/// request for one source window prints the window alone, so its lines can be
/// piped.
pub fn human_text(ctx: &ViewContext<'_>, views: Vec<(&'static str, String)>) -> String {
    let show_headings = views.len() > 1;
    let window_only = matches!(views.as_slice(), [("window", _)]);
    let mut out = Vec::new();
    for (name, rendered) in views {
        if show_headings {
            out.push(format!("== {name} ==").bold().to_string());
        }
        out.push(rendered);
    }
    if !window_only {
        out.push(render_footer(ctx.graph, ctx.stats));
    }
    out.join("\n")
}

/// The requested views as JSON objects keyed by view name.
pub fn json_views(ctx: &ViewContext<'_>, query: &ViewQuery) -> Map<String, Value> {
    let (graph, root, opts) = (ctx.graph, ctx.project_root, &query.options);
    let mut views = Map::new();
    if let Some(arg) = &query.outline {
        views.insert("outline".into(), outline_json(graph, root, arg));
    }
    if let Some(arg) = &query.find_all {
        views.insert("find_all".into(), find_all_json(graph, arg, opts));
    }
    if let Some(arg) = &query.impact {
        views.insert("impact".into(), impact_json(graph, root, arg, opts));
    }
    if let Some(arg) = &query.callers {
        views.insert("callers".into(), callers_json(graph, root, arg, opts));
    }
    if let Some(arg) = &query.callees {
        views.insert("callees".into(), callees_json(graph, root, arg, opts));
    }
    if let Some(arg) = &query.references {
        views.insert("references".into(), references_json(graph, root, arg, opts));
    }
    if let Some(window) = &query.window {
        views.insert("window".into(), window_json(window, ctx.snapshot));
    }
    views
}

/// The top-level `loom-map/2` object around finished views.
pub fn json_payload(ctx: &ViewContext<'_>, views: Map<String, Value>) -> Value {
    json!({
        "schema": MAP_SCHEMA,
        "snapshot": ctx.snapshot.to_json(),
        "views": views,
        "coverage": footer_json(ctx.graph, ctx.stats),
    })
}
