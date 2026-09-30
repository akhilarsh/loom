//! Layers that do not parse or carry another schema are skipped, never served,
//! and `replace_base` overwrites them in place.

use super::*;
use crate::context::source_graph::GRAPH_SCHEMA_VERSION;
use tempfile::TempDir;

fn store(temp: &TempDir) -> GraphStore {
    GraphStore::new(&temp.path().join("cache"), &temp.path().join("work"))
}

fn layer(revision: &str) -> GraphLayer {
    GraphLayer {
        revision: revision.to_string(),
        schema_version: GRAPH_SCHEMA_VERSION,
        ..GraphLayer::default()
    }
}

#[test]
fn read_layer_on_garbage_is_none() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("graph.json");
    fs::write(&path, b"\x00 not json").unwrap();

    assert!(read_layer(&path).unwrap().is_none());
    assert!(read_layer(&temp.path().join("absent.json"))
        .unwrap()
        .is_none());
}

#[test]
fn read_layer_error_other_than_absence_still_fails() {
    let temp = TempDir::new().unwrap();

    // A directory where a file is expected is a read error, not corruption.
    assert!(read_layer(temp.path()).is_err());
}

#[test]
fn load_newest_base_skips_a_corrupt_newest_base() {
    let temp = TempDir::new().unwrap();
    let store = store(&temp);
    store.publish_base("aaaa", &layer("aaaa")).unwrap();
    let newest = store.base_path("bbbb");
    fs::write(&newest, b"garbage").unwrap();
    let later = SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&newest)
        .unwrap()
        .set_modified(later)
        .unwrap();

    let found = store.load_newest_base().unwrap().expect("older base");

    assert_eq!(found.revision, "aaaa");
}

#[test]
fn load_newest_base_skips_a_stale_schema_base() {
    let temp = TempDir::new().unwrap();
    let store = store(&temp);
    store.publish_base("aaaa", &layer("aaaa")).unwrap();
    let stale = GraphLayer {
        schema_version: 0,
        ..layer("bbbb")
    };
    store.publish_base("bbbb", &stale).unwrap();
    let later = SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(store.base_path("bbbb"))
        .unwrap()
        .set_modified(later)
        .unwrap();

    let found = store.load_newest_base().unwrap().expect("current base");

    assert_eq!(found.revision, "aaaa");
}

#[test]
fn load_newest_base_is_none_when_every_base_is_unusable() {
    let temp = TempDir::new().unwrap();
    let store = store(&temp);
    store.publish_base("aaaa", &layer("aaaa")).unwrap();
    fs::write(store.base_path("aaaa"), b"garbage").unwrap();

    assert!(store.load_newest_base().unwrap().is_none());
}

#[test]
fn replace_base_overwrites_a_stale_base_in_place() {
    let temp = TempDir::new().unwrap();
    let store = store(&temp);
    let stale = GraphLayer {
        schema_version: 0,
        ..layer("aaaa")
    };
    store.publish_base("aaaa", &stale).unwrap();

    store.replace_base("aaaa", &layer("aaaa")).unwrap();

    let found = store.load_base("aaaa").unwrap().expect("replaced base");
    assert!(found.has_current_schema());
    assert!(!store.publish_base("aaaa", &stale).unwrap());
}

#[test]
fn replace_base_writes_a_base_that_is_already_gone() {
    let temp = TempDir::new().unwrap();
    let store = store(&temp);

    store
        .replace_base("aaaa", &layer("aaaa"))
        .expect("a base missing at replacement time is not an error");

    let found = read_layer(&store.base_path("aaaa")).unwrap();
    assert_eq!(found, Some(layer("aaaa")));
}
