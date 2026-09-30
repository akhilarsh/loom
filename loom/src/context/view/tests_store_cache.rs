//! Which views a store persists, seeds a relink from and holds in memory: a
//! graph with entries another extractor stamped, the in-process view cache's
//! keys, and view equality.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use serial_test::serial;
use tempfile::TempDir;

use super::build::resolution_runs;
use super::tests_store::{layer_of, store_in, view_file_names, A_RS, B_RS, REV_NEW, REV_OLD};
use super::*;
use crate::context::graph_store::GraphLayer;
use crate::context::source_graph::GRAPH_SCHEMA_VERSION;

const OVERLAY: Option<(&str, &str)> = Some(("plan", "stage"));

/// `layer` as written under the graph schema before the current one.
fn older_schema(mut layer: GraphLayer) -> GraphLayer {
    layer.schema_version = GRAPH_SCHEMA_VERSION - 1;
    layer
}

/// `layer` as an extractor no longer registered stamped it.
fn retired(mut layer: GraphLayer) -> GraphLayer {
    for entry in layer.files.values_mut() {
        for node in &mut entry.nodes {
            node.parser_version = "retired-extractor@0".to_string();
        }
    }
    layer
}

fn assert_equals_cold(view: &ResolvedView, cold: ResolvedView, context: &str) {
    assert_eq!(
        String::from_utf8(canonical_bytes(view).unwrap()).unwrap(),
        String::from_utf8(canonical_bytes(&cold).unwrap()).unwrap(),
        "{context}"
    );
}

#[test]
fn a_base_served_stale_under_an_older_extractor_never_seeds_a_relink() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &retired(layer_of(REV_OLD, &[A_RS, B_RS])))
        .unwrap();

    let served = store.view(REV_OLD, None).unwrap();

    assert_eq!(
        served.graph.files.len(),
        2,
        "the stale base is still served"
    );
    assert!(
        view_file_names(&store).is_empty(),
        "a view claiming the current extractors was persisted over retired entries"
    );

    store
        .publish_base(REV_NEW, &layer_of(REV_NEW, &[A_RS, B_RS]))
        .unwrap();
    let view = store.view(REV_NEW, None).unwrap();

    let cold = build_cold(
        store.resolved(REV_NEW, None).unwrap(),
        ViewIdentity::current(REV_NEW, ""),
    );
    assert_equals_cold(&view, cold, "relinked from the retired entries");
}

#[test]
fn an_overlay_view_never_relinks_from_a_base_view_of_retired_entries() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &retired(layer_of(REV_OLD, &[A_RS, B_RS])))
        .unwrap();
    let mut overlay = layer_of(REV_OLD, &[A_RS, B_RS]);
    overlay.generation = "gen-1".to_string();
    store.save_overlay("plan", "stage", &overlay).unwrap();

    let view = store.view(REV_OLD, OVERLAY).unwrap();

    let cold = build_cold(
        store.resolved(REV_OLD, OVERLAY).unwrap(),
        ViewIdentity::current(REV_OLD, "gen-1"),
    );
    assert_equals_cold(&view, cold, "the overlay copied the retired base entries");
}

#[test]
fn an_overlay_view_without_a_generation_is_never_held_as_the_base_view() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &layer_of(REV_OLD, &[A_RS]))
        .unwrap();
    store
        .save_overlay("plan", "stage", &layer_of(REV_OLD, &[B_RS]))
        .unwrap();

    store.materialize_view(REV_OLD, OVERLAY).unwrap();
    let base = store.view(REV_OLD, None).unwrap();

    assert!(
        !base.graph.files.contains_key(B_RS.0),
        "the overlay view was served as the base view"
    );
}

#[test]
fn a_request_whose_overlay_layer_is_gone_is_held_as_the_base_view() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &layer_of(REV_OLD, &[A_RS, B_RS]))
        .unwrap();

    store.materialize_view(REV_OLD, OVERLAY).unwrap();
    let before = resolution_runs();
    let base = store.view(REV_OLD, None).unwrap();

    assert_eq!(resolution_runs(), before, "the held view was not found");
    assert_eq!(base.origin, ViewOrigin::Built, "the view file was parsed");
}

#[test]
fn an_overlay_build_hands_the_base_view_back_to_the_cache() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &layer_of(REV_OLD, &[A_RS]))
        .unwrap();
    let mut overlay = layer_of(REV_OLD, &[B_RS]);
    overlay.generation = "gen-1".to_string();
    store.save_overlay("plan", "stage", &overlay).unwrap();

    store.materialize_view(REV_OLD, None).unwrap();
    store.materialize_view(REV_OLD, OVERLAY).unwrap();
    let base = store.view(REV_OLD, None).unwrap();

    assert_eq!(
        base.origin,
        ViewOrigin::Built,
        "the base view was parsed again after the overlay build took it"
    );
}

#[test]
fn removing_a_revisions_views_forgets_the_ones_held_in_memory() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &layer_of(REV_OLD, &[A_RS, B_RS]))
        .unwrap();
    store.materialize_view(REV_OLD, None).unwrap();

    store.remove_views(REV_OLD);
    let before = resolution_runs();
    store.view(REV_OLD, None).unwrap();

    assert!(
        resolution_runs() > before,
        "a view held for a revision whose views were removed was served"
    );
}

#[test]
fn view_equality_ignores_the_origin() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &layer_of(REV_OLD, &[A_RS, B_RS]))
        .unwrap();

    let built = store.view(REV_OLD, None).unwrap();
    let loaded = store_in(temp.path()).view(REV_OLD, None).unwrap();

    assert_eq!(built.origin, ViewOrigin::Built);
    assert_eq!(loaded.origin, ViewOrigin::Materialized);
    assert_eq!(built, loaded);
}

#[test]
fn a_base_written_under_an_older_schema_is_never_persisted_nor_seeds_a_relink() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &older_schema(layer_of(REV_OLD, &[A_RS, B_RS])))
        .unwrap();

    let served = store.view(REV_OLD, None).unwrap();

    assert_eq!(
        served.graph.files.len(),
        2,
        "the older-schema base is still served"
    );
    assert!(
        view_file_names(&store).is_empty(),
        "a view claiming the current schema was persisted over an older-schema base"
    );

    store
        .publish_base(REV_NEW, &layer_of(REV_NEW, &[A_RS, B_RS]))
        .unwrap();
    let view = store.view(REV_NEW, None).unwrap();

    let cold = build_cold(
        store.resolved(REV_NEW, None).unwrap(),
        ViewIdentity::current(REV_NEW, ""),
    );
    assert_equals_cold(&view, cold, "relinked from the older-schema base");
}

#[test]
fn an_overlay_written_under_an_older_schema_is_never_persisted() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &layer_of(REV_OLD, &[A_RS]))
        .unwrap();
    let mut overlay = older_schema(layer_of(REV_OLD, &[B_RS]));
    overlay.generation = "gen-1".to_string();
    store.save_overlay("plan", "stage", &overlay).unwrap();

    let view = store.view(REV_OLD, OVERLAY).unwrap();

    let identity = ViewIdentity::current(REV_OLD, "gen-1");
    assert!(
        view.graph.files.contains_key(B_RS.0),
        "the older-schema overlay is still served"
    );
    assert!(
        !store.view_path(&identity, OVERLAY).exists(),
        "an overlay view claiming the current schema was persisted over an older-schema layer"
    );
    assert!(
        view_file_names(&store).is_empty(),
        "an older-schema overlay seeded a base view"
    );
    let cold = build_cold(store.resolved(REV_OLD, OVERLAY).unwrap(), identity);
    assert_equals_cold(&view, cold, "built from the older-schema overlay");
}

#[test]
#[serial]
fn a_persisted_view_replaces_the_one_a_denied_write_kept_in_memory() {
    let temp = TempDir::new().unwrap();
    let store = store_in(temp.path());
    store
        .publish_base(REV_OLD, &layer_of(REV_OLD, &[A_RS]))
        .unwrap();
    let mut overlay = layer_of(REV_OLD, &[B_RS]);
    overlay.generation = "gen-1".to_string();
    store.save_overlay("plan", "stage", &overlay).unwrap();
    let overlay_dir = store.overlay_dir("plan", "stage");
    fs::set_permissions(&overlay_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let enforced = fs::write(overlay_dir.join(".probe"), b"x").is_err();

    let kept = store.view(REV_OLD, OVERLAY);
    let held = store.fell_back();
    fs::set_permissions(&overlay_dir, fs::Permissions::from_mode(0o755)).unwrap();

    if !enforced {
        eprintln!("SKIP: this environment does not enforce 0o555 directory permissions");
        return;
    }
    kept.unwrap();
    assert!(held, "the denied overlay view write was not kept in memory");

    overlay.generation = "gen-2".to_string();
    store.save_overlay("plan", "stage", &overlay).unwrap();
    store.view(REV_OLD, OVERLAY).unwrap();

    let identity = ViewIdentity::current(REV_OLD, "gen-2");
    assert!(store.view_path(&identity, OVERLAY).is_file());
    assert!(
        !store.fell_back(),
        "a view kept in memory shadows the file that replaced it"
    );
}
