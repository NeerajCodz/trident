//! Adjacency-list graph index: `node_rid → [Edge { to, label, properties }]`.
//!
//! Nodes are identified by their [`RecordId`] in the primary data store.
//! The index stores only the graph connectivity (edges); the node body is
//! retrieved from the [`RecordStore`][crate::store::RecordStore] when
//! needed.  No node data is duplicated inside this index.
//!
//! Edges are directed.  Bi-directional links require two calls to
//! [`AdjacencyIndex::add_edge`] (one for each direction).
//!
//! The index maintains both a forward adjacency map (source → targets) and a
//! reverse adjacency map (target → sources) for efficient inbound traversal.
//! Each edge can carry arbitrary key-value properties and an optional weight
//! used by [`AdjacencyIndex::shortest_path_weighted`].

use crate::errors::{Result, TridentError};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::path::{Path, PathBuf};

/// A directed edge from one node to another with an optional opaque label
/// and arbitrary key-value properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// The target node's [`RecordId`] in the primary data store.
    pub to: RecordId,
    /// Opaque edge label (e.g. `b"follows"`, `b"friend"`, relationship type).
    pub label: Vec<u8>,
    /// Arbitrary key-value properties attached to this edge.
    ///
    /// The special key `"weight"` is used by
    /// [`AdjacencyIndex::shortest_path_weighted`] as the numeric edge weight
    /// (defaults to `1.0` if absent).
    pub properties: BTreeMap<String, serde_json::Value>,
}

impl Edge {
    /// Return the numeric weight of this edge.
    ///
    /// Looks for a `"weight"` key in [`Edge::properties`]; returns `1.0` if
    /// absent or non-numeric.
    pub fn weight(&self) -> f64 {
        match self.properties.get("weight") {
            Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(1.0),
            _ => 1.0,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    adjacency: HashMap<u64, Vec<Edge>>,
    #[serde(default)]
    reverse_index: HashMap<u64, Vec<u64>>,
}

const ADJ_SNAPSHOT_MAGIC: u32 = 0x4144_4a32;
const ADJ_SNAPSHOT_VERSION: u8 = 2;

/// Adjacency-list index for graph workloads.
///
/// Stores `source_node_rid → [Edge { to, label, properties }]`.  The actual
/// node body bytes live in the [`RecordStore`][crate::store::RecordStore] and
/// are never duplicated here.
///
/// Maintains both forward (`adjacency`) and reverse (`reverse_index`) maps so
/// that both outbound and inbound traversals are O(1) per hop.
pub struct AdjacencyIndex {
    name: String,
    dir: PathBuf,
    /// Outgoing edges keyed by source node's RID (as raw u64).
    adjacency: HashMap<u64, Vec<Edge>>,
    /// Inbound index: target RID → list of source RIDs.
    reverse_index: HashMap<u64, Vec<u64>>,
}

impl AdjacencyIndex {
    /// Open or create an adjacency index named `name` inside `dir`.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let path = Self::snapshot_path(&dir, &name);
        let (adjacency, reverse_index) = if path.exists() {
            let bytes = std::fs::read(&path)?;
            let on_disk: OnDisk = if looks_like_binary_snapshot(&bytes) {
                decode_binary_snapshot(&bytes, &path)?
            } else {
                serde_json::from_slice(&bytes)?
            };
            // Rebuild reverse_index from adjacency if the snapshot didn't
            // carry one (v1 snapshots) or if it was empty.
            let reverse_index = if on_disk.reverse_index.is_empty() {
                build_reverse_index(&on_disk.adjacency)
            } else {
                on_disk
                    .reverse_index
                    .into_iter()
                    .map(|(k, v)| (k, v))
                    .collect()
            };
            (on_disk.adjacency, reverse_index)
        } else {
            (HashMap::new(), HashMap::new())
        };

        Ok(Self {
            name,
            dir,
            adjacency,
            reverse_index,
        })
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.adjidx"))
    }

    /// Add a directed edge from `from` to `to` with the given `label` and
    /// optional `properties`.
    ///
    /// Duplicate edges (identical `from`, `to`, and `label`) are silently
    /// ignored.  The reverse index is updated atomically.
    pub fn add_edge(
        &mut self,
        from: RecordId,
        label: &[u8],
        to: RecordId,
        properties: BTreeMap<String, serde_json::Value>,
    ) -> Result<()> {
        let edge = Edge {
            to,
            label: label.to_vec(),
            properties,
        };
        let edges = self.adjacency.entry(from.0).or_default();
        if !edges.iter().any(|e| e.to == to && e.label == edge.label) {
            edges.push(edge);
            // Maintain reverse index.
            self.reverse_index.entry(to.0).or_default().push(from.0);
        }
        Ok(())
    }

    /// Add a directed edge with no properties (convenience).
    pub fn add_edge_simple(&mut self, from: RecordId, label: &[u8], to: RecordId) -> Result<()> {
        self.add_edge(from, label, to, BTreeMap::new())
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
        self.add_edge_simple(left, left_to_right_label, right)?;
        self.add_edge_simple(right, right_to_left_label, left)?;
        Ok(())
    }

    /// Remove all edges from `from` to `to` (regardless of label).
    ///
    /// Updates both the forward adjacency list and the reverse index.
    pub fn remove_edges(&mut self, from: RecordId, to: RecordId) {
        if let Some(edges) = self.adjacency.get_mut(&from.0) {
            let before = edges.len();
            edges.retain(|e| e.to != to);
            if edges.len() < before {
                // Update reverse index: remove `from` from `to`'s inbound list.
                if let Some(sources) = self.reverse_index.get_mut(&to.0) {
                    sources.retain(|&s| s != from.0);
                    if sources.is_empty() {
                        self.reverse_index.remove(&to.0);
                    }
                }
            }
        }
    }

    /// Return all outgoing edges from `from`.
    pub fn neighbors(&self, from: RecordId) -> Vec<Edge> {
        self.adjacency.get(&from.0).cloned().unwrap_or_default()
    }

    /// Return all incoming edges to `to` (sources that point to `to`).
    pub fn inbound_neighbors(&self, to: RecordId) -> Vec<RecordId> {
        self.reverse_index
            .get(&to.0)
            .map(|rids| rids.iter().map(|&r| RecordId(r)).collect())
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

    // ------------------------------------------------------------------
    // Traversal algorithms
    // ------------------------------------------------------------------

    /// Breadth-first traversal starting from `start`, up to `max_hops` deep.
    ///
    /// Returns all reachable [`RecordId`]s in BFS order (including `start`).
    pub fn bfs(&self, start: RecordId, max_hops: usize) -> Vec<RecordId> {
        let mut seen = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::from([(start, 0usize)]);
        let mut output = Vec::new();
        while let Some((node, depth)) = queue.pop_front() {
            if !seen.insert(node) || depth > max_hops {
                continue;
            }
            output.push(node);
            if depth == max_hops {
                continue;
            }
            for edge in self.adjacency.get(&node.0).cloned().unwrap_or_default() {
                queue.push_back((edge.to, depth + 1));
            }
        }
        output
    }

    /// Depth-first traversal starting from `start`, up to `max_hops` deep.
    ///
    /// Returns all reachable [`RecordId`]s in DFS pre-order (including
    /// `start`).  Cycles are detected and skipped.
    pub fn dfs(&self, start: RecordId, max_hops: usize) -> Vec<RecordId> {
        let mut seen = std::collections::HashSet::new();
        let mut output = Vec::new();
        self.dfs_inner(start, 0, max_hops, &mut seen, &mut output);
        output
    }

    fn dfs_inner(
        &self,
        node: RecordId,
        depth: usize,
        max_hops: usize,
        seen: &mut std::collections::HashSet<u64>,
        output: &mut Vec<RecordId>,
    ) {
        if depth > max_hops || !seen.insert(node.0) {
            return;
        }
        output.push(node);
        if depth == max_hops {
            return;
        }
        for edge in self.adjacency.get(&node.0).cloned().unwrap_or_default() {
            self.dfs_inner(edge.to, depth + 1, max_hops, seen, output);
        }
    }

    /// Unweighted shortest path (BFS-based) from `start` to `target`.
    ///
    /// Returns the path as a list of [`RecordId`]s including both endpoints,
    /// or an error if no path exists.
    pub fn shortest_path(&self, start: RecordId, target: RecordId) -> Result<Vec<RecordId>> {
        if start == target {
            return Ok(vec![start]);
        }
        let mut queue =
            std::collections::VecDeque::from([(start, vec![start])]);
        let mut seen = std::collections::HashSet::new();
        while let Some((node, path)) = queue.pop_front() {
            if !seen.insert(node.0) {
                continue;
            }
            for edge in self.adjacency.get(&node.0).cloned().unwrap_or_default() {
                let mut next_path = path.clone();
                next_path.push(edge.to);
                if edge.to == target {
                    return Ok(next_path);
                }
                queue.push_back((edge.to, next_path));
            }
        }
        Err(TridentError::Query("path not found".into()))
    }

    /// Weighted shortest path from `start` to `target` using Dijkstra's
    /// algorithm.
    ///
    /// Edge weights are read from the `"weight"` property of each edge
    /// (defaults to `1.0` when absent).  Returns the path as a list of
    /// [`RecordId`]s including both endpoints, or an error if no path exists.
    ///
    /// # Complexity
    ///
    /// O((V + E) log V) where V = visited nodes, E = examined edges.
    pub fn shortest_path_weighted(
        &self,
        start: RecordId,
        target: RecordId,
    ) -> Result<Vec<RecordId>> {
        if start == target {
            return Ok(vec![start]);
        }

        // Min-heap via Reverse: (distance, node_rid).
        let mut heap: BinaryHeap<Reverse<(OrderedFloat, u64)>> = BinaryHeap::new();
        let mut dist: HashMap<u64, f64> = HashMap::new();
        let mut prev: HashMap<u64, u64> = HashMap::new();

        dist.insert(start.0, 0.0);
        heap.push(Reverse((OrderedFloat(0.0), start.0)));

        while let Some(Reverse((OrderedFloat(d), rid))) = heap.pop() {
            if rid == target.0 {
                // Reconstruct path.
                let mut path = Vec::new();
                let mut cur = target.0;
                while cur != start.0 {
                    path.push(RecordId(cur));
                    cur = *prev.get(&cur).unwrap();
                }
                path.push(start);
                path.reverse();
                return Ok(path);
            }

            // Skip stale entries.
            if d > *dist.get(&rid).unwrap_or(&f64::INFINITY) {
                continue;
            }

            for edge in self.adjacency.get(&rid).cloned().unwrap_or_default() {
                let new_dist = d + edge.weight();
                if new_dist < *dist.get(&edge.to.0).unwrap_or(&f64::INFINITY) {
                    dist.insert(edge.to.0, new_dist);
                    prev.insert(edge.to.0, rid);
                    heap.push(Reverse((OrderedFloat(new_dist), edge.to.0)));
                }
            }
        }

        Err(TridentError::Query("path not found".into()))
    }

    // ------------------------------------------------------------------
    // Persistence
    // ------------------------------------------------------------------

    /// Persist the adjacency list to disk.
    pub fn flush(&mut self) -> Result<()> {
        let on_disk = OnDisk {
            adjacency: self.adjacency.clone(),
            reverse_index: self
                .reverse_index
                .iter()
                .map(|(&k, v)| (k, v.clone()))
                .collect(),
        };
        let bytes = encode_binary_snapshot(&on_disk);
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper types for Dijkstra
// ---------------------------------------------------------------------------

/// Total-order wrapper for `f64` so it can be used in a `BinaryHeap`.
/// NaN is treated as greater than every other value.
#[derive(Clone, Copy, Debug)]
struct OrderedFloat(f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a reverse index from the forward adjacency map.
fn build_reverse_index(adjacency: &HashMap<u64, Vec<Edge>>) -> HashMap<u64, Vec<u64>> {
    let mut reverse: HashMap<u64, Vec<u64>> = HashMap::new();
    for (&from, edges) in adjacency {
        for edge in edges {
            reverse.entry(edge.to.0).or_default().push(from);
        }
    }
    reverse
}

fn looks_like_binary_snapshot(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == ADJ_SNAPSHOT_MAGIC
}

fn encode_binary_snapshot(on_disk: &OnDisk) -> Vec<u8> {
    let mut payload = BinaryWriter::new();

    // Forward adjacency: sorted for deterministic output.
    let mut nodes: Vec<(&u64, &Vec<Edge>)> = on_disk.adjacency.iter().collect();
    nodes.sort_by_key(|(rid, _)| **rid);
    payload.write_u32(nodes.len() as u32);
    for (rid, edges) in nodes {
        payload.write_u64(*rid);
        payload.write_u32(edges.len() as u32);
        for edge in edges {
            payload.write_u64(edge.to.0);
            payload.write_len_bytes(&edge.label);
            // Serialize properties as JSON bytes.
            let props_json = serde_json::to_vec(&edge.properties).unwrap_or_default();
            payload.write_len_bytes(&props_json);
        }
    }

    // Reverse index.
    let mut rev: Vec<(&u64, &Vec<u64>)> = on_disk.reverse_index.iter().collect();
    rev.sort_by_key(|(rid, _)| **rid);
    payload.write_u32(rev.len() as u32);
    for (target, sources) in rev {
        payload.write_u64(*target);
        payload.write_u32(sources.len() as u32);
        for &src in sources {
            payload.write_u64(src);
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
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated adjacency snapshot header".to_string(),
        });
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != ADJ_SNAPSHOT_MAGIC {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "bad adjacency snapshot magic".to_string(),
        });
    }
    let version = bytes[4];
    if version != 1 && version != 2 {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!("unsupported adjacency snapshot version {version}"),
        });
    }
    let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    let expected_crc = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    if bytes.len() < 13 + payload_len {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated adjacency snapshot payload".to_string(),
        });
    }
    let payload = &bytes[13..13 + payload_len];
    let actual_crc = crc32c(payload);
    if actual_crc != expected_crc {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!(
                "adjacency snapshot checksum mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
            ),
        });
    }
    let mut reader = BinaryReader::new(payload, source.to_path_buf());

    // Forward adjacency.
    let node_count = reader.read_u32()? as usize;
    let mut adjacency = HashMap::with_capacity(node_count);
    for _ in 0..node_count {
        let from = reader.read_u64()?;
        let edge_count = reader.read_u32()? as usize;
        let mut edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            let to = RecordId(reader.read_u64()?);
            let label = reader.read_len_bytes()?;
            let properties = if version >= 2 {
                let props_bytes = reader.read_len_bytes()?;
                serde_json::from_slice(&props_bytes).unwrap_or_default()
            } else {
                BTreeMap::new()
            };
            edges.push(Edge {
                to,
                label,
                properties,
            });
        }
        adjacency.insert(from, edges);
    }

    // Reverse index (v2 only; v1 returns empty and will be rebuilt on open).
    let reverse_index = if version >= 2 {
        let rev_count = reader.read_u32()? as usize;
        let mut reverse = HashMap::with_capacity(rev_count);
        for _ in 0..rev_count {
            let target = reader.read_u64()?;
            let src_count = reader.read_u32()? as usize;
            let mut sources = Vec::with_capacity(src_count);
            for _ in 0..src_count {
                sources.push(reader.read_u64()?);
            }
            reverse.insert(target, sources);
        }
        reverse
    } else {
        HashMap::new()
    };

    Ok(OnDisk {
        adjacency,
        reverse_index,
    })
}
