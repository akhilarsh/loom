//! The dependency index: each lookup key to the edges whose resolution
//! consulted it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::context::resolve::{EdgeKeys, EdgeRef};

/// Lookup key (`name:`, `ns:` or `pathset:`) to the edges that consulted it,
/// sorted. Holds no key with an empty list, so an index built cold and one
/// maintained by relink serialize alike.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DependencyIndex {
    by_key: BTreeMap<String, BTreeSet<EdgeRef>>,
}

impl DependencyIndex {
    /// Invert the keys each edge consulted.
    pub(crate) fn from_edge_keys(keys: EdgeKeys) -> Self {
        let mut index = Self::default();
        index.record(keys);
        index
    }

    /// The edges whose resolution consulted `key`, sorted.
    pub fn edges(&self, key: &str) -> impl Iterator<Item = &EdgeRef> + '_ {
        self.by_key.get(key).into_iter().flatten()
    }

    /// Add the keys each edge consulted.
    pub(crate) fn record(&mut self, keys: EdgeKeys) {
        for (edge, consulted) in keys {
            for key in consulted {
                self.by_key.entry(key).or_default().insert(edge.clone());
            }
        }
    }

    /// Remove every edge `stale` accepts, then every key left with no edge.
    pub(crate) fn forget(&mut self, stale: impl Fn(&EdgeRef) -> bool) {
        for edges in self.by_key.values_mut() {
            edges.retain(|edge| !stale(edge));
        }
        self.by_key.retain(|_, edges| !edges.is_empty());
    }
}
