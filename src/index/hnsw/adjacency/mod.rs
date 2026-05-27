//! Adjacency-list graph index: `node_rid → [Edge { to, label }]`.
//!
//! Nodes are identified by their [`RecordId`] in the primary data store.
//! The index stores only the graph connectivity (edges); the node body is
//! retrieved from the [`RecordStore`][crate::store::RecordStore] when
//! needed.  No node data is duplicated inside this index.
//!
//! Edges are directed.  Bi-directional links require two calls to
//! [`AdjacencyIndex::add_edge`] (one for each direction).

pub mod binary;

use crate::errors::{PraxisError, Result};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
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

const ADJ_SNAPSHOT_MAGIC: u32 = 0x4144_4a32;
const ADJ_SNAPSHOT_VERSION: u8 = 1;

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
            let on_disk: OnDisk = if looks_like_binary_snapshot(&bytes) {
                decode_binary_snapshot(&bytes, &path)?
            } else {
                serde_json::from_slice(&bytes)?
            };
            on_disk.adjacency
        } else {
            HashMap::new()
        };

        Ok(Self {
            name,
            dir,
            adjacency,
        })
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

    /// Atomically insert two directed edges (`left -> right`, `right -> left`).
    ///
    /// If either edge already exists it is treated as idempotent.
    pub fn add_bidirectional_edge(
        &mut self,
        left: RecordId,
        left_to_right_label: &[u8],
        right: RecordId,
        right_to_left_label: &[u8],
    ) -> Result<()> {
        self.add_edge(left, left_to_right_label, right)?;
        self.add_edge(right, right_to_left_label, left)?;
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
        self.adjacency.get(&from.0).cloned().unwrap_or_default()
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
        let bytes = encode_binary_snapshot(&on_disk);
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }
}

fn looks_like_binary_snapshot(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == ADJ_SNAPSHOT_MAGIC
}

fn encode_binary_snapshot(on_disk: &OnDisk) -> Vec<u8> {
    let mut payload = BinaryWriter::new();
    let mut nodes: Vec<(&u64, &Vec<Edge>)> = on_disk.adjacency.iter().collect();
    nodes.sort_by_key(|(rid, _)| **rid);
    payload.write_u32(nodes.len() as u32);
    for (rid, edges) in nodes {
        payload.write_u64(*rid);
        payload.write_u32(edges.len() as u32);
        for edge in edges {
            payload.write_u64(edge.to.0);
            payload.write_len_bytes(&edge.label);
        }
    }
    let payload = payload.into_inner();
    let mut out = BinaryWriter::new();
    out.write_u32(ADJ_SNAPSHOT_MAGIC);
    out.write_u8(ADJ_SNAPSHOT_VERSION);
    out.write_u32(payload.len() as u32);
    out.write_u32(crc32c(&payload));
    out.write_bytes(&payload);
    out.into_inner()
}

fn decode_binary_snapshot(bytes: &[u8], source: &Path) -> Result<OnDisk> {
    if bytes.len() < 13 {
        return Err(PraxisError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated adjacency snapshot header".to_string(),
        });
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != ADJ_SNAPSHOT_MAGIC {
        return Err(PraxisError::Corrupt {
            path: source.to_path_buf(),
            reason: "bad adjacency snapshot magic".to_string(),
        });
    }
    if bytes[4] != ADJ_SNAPSHOT_VERSION {
        return Err(PraxisError::Corrupt {
            path: source.to_path_buf(),
            reason: format!("unsupported adjacency snapshot version {}", bytes[4]),
        });
    }
    let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    let expected_crc = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    if bytes.len() < 13 + payload_len {
        return Err(PraxisError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated adjacency snapshot payload".to_string(),
        });
    }
    let payload = &bytes[13..13 + payload_len];
    let actual_crc = crc32c(payload);
    if actual_crc != expected_crc {
        return Err(PraxisError::Corrupt {
            path: source.to_path_buf(),
            reason: format!(
                "adjacency snapshot checksum mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
            ),
        });
    }
    let mut reader = BinaryReader::new(payload, source.to_path_buf());
    let node_count = reader.read_u32()? as usize;
    let mut adjacency = HashMap::with_capacity(node_count);
    for _ in 0..node_count {
        let from = reader.read_u64()?;
        let edge_count = reader.read_u32()? as usize;
        let mut edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            edges.push(Edge {
                to: RecordId(reader.read_u64()?),
                label: reader.read_len_bytes()?,
            });
        }
        adjacency.insert(from, edges);
    }
    Ok(OnDisk { adjacency })
}
