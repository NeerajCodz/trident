//! Adjacency-list graph index: `node_rid → [Edge { to, label }]`.
//!
//! Nodes are identified by their [`RecordId`] in the primary data store.
//! The index stores only the graph connectivity (edges); the node body is
//! retrieved from the [`RecordStore`][crate::store::RecordStore] when
//! needed.  No node data is duplicated inside this index.
//!
//! Edges are directed.  Bi-directional links require two calls to
//! [`AdjacencyIndex::add_edge`] (one for each direction).

use crate::errors::Result;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A directed edge from one node to another with an optional opaque label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// The target node's [`RecordId`] in the primary data store.
    pub to: RecordId,
    /// Opaque edge label (e.g. `b"follows"`, `b"friend"`, relationship type).
    pub label: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    adjacency: HashMap<u64, Vec<Edge>>,
}

/// Adjacency-list index for graph workloads.
///
/// Stores `source_node_rid → [Edge { to, label }]`.  The actual node body
/// bytes live in the [`RecordStore`][crate::store::RecordStore] and are
/// never duplicated here.
pub struct AdjacencyIndex {
    name: String,
    dir: PathBuf,
    /// Outgoing edges keyed by source node's RID (as raw u64).
    adjacency: HashMap<u64, Vec<Edge>>,
}

impl AdjacencyIndex {
    /// Open or create an adjacency index named `name` inside `dir`.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let path = Self::snapshot_path(&dir, &name);
        let adjacency = if path.exists() {
            let bytes = std::fs::read(&path)?;
            let on_disk: OnDisk = serde_json::from_slice(&bytes)?;
            on_disk.adjacency
        } else {
            HashMap::new()
        };

        Ok(Self { name, dir, adjacency })
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.adjidx"))
    }

    /// Add a directed edge from `from` to `to` with the given `label`.
    ///
    /// Duplicate edges (identical `from`, `to`, and `label`) are silently
    /// ignored.
    pub fn add_edge(&mut self, from: RecordId, label: &[u8], to: RecordId) -> Result<()> {
        let edge = Edge {
            to,
            label: label.to_vec(),
        };
        let edges = self.adjacency.entry(from.0).or_default();
        if !edges.contains(&edge) {
            edges.push(edge);
        }
        Ok(())
    }

    /// Remove all edges from `from` to `to` (regardless of label).
    pub fn remove_edges(&mut self, from: RecordId, to: RecordId) {
        if let Some(edges) = self.adjacency.get_mut(&from.0) {
            edges.retain(|e| e.to != to);
        }
    }

    /// Return all outgoing edges from `from`.
    pub fn neighbors(&self, from: RecordId) -> Vec<Edge> {
        self.adjacency
            .get(&from.0)
            .cloned()
            .unwrap_or_default()
    }

    /// Return the [`RecordId`]s of all neighbors reachable from `from`
    /// through edges with the given `label`.
    pub fn neighbors_with_label(&self, from: RecordId, label: &[u8]) -> Vec<RecordId> {
        self.adjacency
            .get(&from.0)
            .map(|edges| {
                edges
                    .iter()
                    .filter(|e| e.label.as_slice() == label)
                    .map(|e| e.to)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Persist the adjacency list to disk.
    pub fn flush(&mut self) -> Result<()> {
        let on_disk = OnDisk {
            adjacency: self.adjacency.clone(),
        };
        let bytes = serde_json::to_vec(&on_disk)?;
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }
}
