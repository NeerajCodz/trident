//! Vector approximate nearest-neighbor index with HNSW-like graph search.
//!
//! This module keeps one graph per vector dimensionality. Inserts build
//! bounded-degree small-world links with layered navigation and persisted
//! graph state.

use crate::errors::{Result, TridentError};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HnswConfig {
    pub max_connections: usize,
    pub ef_construction: usize,
    pub ef_search: usize,
    pub max_level: u8,
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            max_connections: 16,
            ef_construction: 64,
            ef_search: 64,
            max_level: 16,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GraphNode {
    rid: RecordId,
    vector: Vec<f32>,
    level: u8,
    /// Per-layer adjacency lists; index 0 is the base layer.
    neighbors: Vec<Vec<usize>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct HnswGraph {
    nodes: Vec<GraphNode>,
    entry_point: Option<usize>,
    max_level: u8,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    config: HnswConfig,
    graphs: HashMap<usize, HnswGraph>,
}

#[derive(Serialize, Deserialize)]
struct OnDiskV1 {
    vectors: Vec<(Vec<f32>, RecordId)>,
}

const HNSW_SNAPSHOT_MAGIC: u32 = 0x484e_5332;
const HNSW_SNAPSHOT_VERSION: u8 = 1;

/// Vector ANN index backed by a persisted multi-layer graph.
pub struct HnswIndex {
    name: String,
    dir: Option<PathBuf>,
    config: HnswConfig,
    graphs: HashMap<usize, HnswGraph>,
}

impl HnswIndex {
    /// Create a new in-memory vector index with default HNSW parameters.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_config(name, HnswConfig::default())
    }

    /// Create a new in-memory vector index with explicit configuration.
    pub fn with_config(name: impl Into<String>, config: HnswConfig) -> Self {
        Self {
            name: name.into(),
            dir: None,
            config,
            graphs: HashMap::new(),
        }
    }

    /// Open or create a persisted vector index.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with_config(name, dir, HnswConfig::default())
    }

    /// Open or create a persisted vector index with explicit configuration.
    pub fn open_with_config(
        name: impl Into<String>,
        dir: impl Into<PathBuf>,
        config: HnswConfig,
    ) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let path = snapshot_path(&dir, &name);
        if !path.exists() {
            return Ok(Self {
                name,
                dir: Some(dir),
                config,
                graphs: HashMap::new(),
            });
        }

        let bytes = std::fs::read(path)?;
        if looks_like_binary_snapshot(&bytes) {
            let on_disk = decode_binary_snapshot(&bytes, &snapshot_path(&dir, &name))?;
            return Ok(Self {
                name,
                dir: Some(dir),
                config: on_disk.config,
                graphs: on_disk.graphs,
            });
        }
        if let Ok(on_disk) = serde_json::from_slice::<OnDisk>(&bytes) {
            return Ok(Self {
                name,
                dir: Some(dir),
                config: on_disk.config,
                graphs: on_disk.graphs,
            });
        }

        // Backward compatibility with prior scaffold snapshot shape.
        let legacy: OnDiskV1 = serde_json::from_slice(&bytes)?;
        let mut index = Self {
            name,
            dir: Some(dir),
            config,
            graphs: HashMap::new(),
        };
        for (vector, rid) in legacy.vectors {
            index.insert(vector, rid)?;
        }
        Ok(index)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn config(&self) -> HnswConfig {
        self.config
    }

    /// Index `vector` as the ANN key for `rid`.
    pub fn insert(&mut self, vector: Vec<f32>, rid: RecordId) -> Result<()> {
        let dim = vector.len();
        let level = sample_level(rid, self.len() as u64, self.config.max_level);
        let graph = self.graphs.entry(dim).or_default();
        let node_id = graph.nodes.len();
        graph.nodes.push(GraphNode {
            rid,
            vector: vector.clone(),
            level,
            neighbors: vec![Vec::new(); level as usize + 1],
        });

        let Some(mut entry) = graph.entry_point else {
            graph.entry_point = Some(node_id);
            graph.max_level = level;
            return Ok(());
        };

        // Greedy descent from current top layer to insertion level.
        let mut layer = graph.max_level as i32;
        while layer > level as i32 {
            entry = greedy_search_layer(graph, &vector, entry, layer as u8);
            layer -= 1;
        }

        let max_insert_layer = std::cmp::min(level, graph.max_level);
        for current_layer in (0..=max_insert_layer).rev() {
            let ef = self.config.ef_construction.max(self.config.max_connections);
            let candidates = search_layer(graph, &vector, &[entry], ef, current_layer);
            let selected = select_best(candidates, self.config.max_connections);
            for neighbor in selected {
                connect_bidirectional(
                    graph,
                    node_id,
                    neighbor,
                    current_layer,
                    self.config.max_connections,
                );
            }
            if let Some(next_entry) = graph.nodes[node_id].neighbors[current_layer as usize].first()
            {
                entry = *next_entry;
            }
        }

        if level > graph.max_level {
            graph.entry_point = Some(node_id);
            graph.max_level = level;
        }
        Ok(())
    }

    /// Return the top-`k` nearest neighbors to `query` as `(rid, distance)`.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<(RecordId, f32)> {
        if k == 0 {
            return Vec::new();
        }
        let Some(graph) = self.graphs.get(&query.len()) else {
            return Vec::new();
        };
        let Some(mut entry) = graph.entry_point else {
            return Vec::new();
        };

        for layer in (1..=graph.max_level).rev() {
            entry = greedy_search_layer(graph, query, entry, layer);
        }
        let ef = self.config.ef_search.max(k);
        let candidates = search_layer(graph, query, &[entry], ef, 0);
        let mut top = select_best(candidates, k)
            .into_iter()
            .map(|id| {
                (
                    graph.nodes[id].rid,
                    l2_distance(query, &graph.nodes[id].vector),
                )
            })
            .collect::<Vec<_>>();
        top.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        top
    }

    /// Total number of indexed vectors across all dimensions.
    pub fn len(&self) -> usize {
        self.graphs.values().map(|graph| graph.nodes.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Persist graph state when opened in disk-backed mode.
    pub fn flush(&mut self) -> Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        let on_disk = OnDisk {
            config: self.config,
            graphs: self.graphs.clone(),
        };
        let bytes = encode_binary_snapshot(&on_disk);
        std::fs::write(snapshot_path(dir, &self.name), bytes)?;
        Ok(())
    }

    pub fn snapshot_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| snapshot_path(dir, &self.name))
    }
}

fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.hnsw"))
}

fn sample_level(rid: RecordId, ordinal: u64, max_level: u8) -> u8 {
    let mut x = splitmix64(rid.0 ^ (ordinal.wrapping_mul(0x9e37_79b9_7f4a_7c15)));
    let mut level = 0u8;
    // Geometric-ish distribution: roughly quarter chance to climb one more level.
    while level < max_level && (x & 0b11) == 0 {
        level = level.saturating_add(1);
        x >>= 2;
    }
    level
}

fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn greedy_search_layer(graph: &HnswGraph, query: &[f32], entry: usize, layer: u8) -> usize {
    let mut current = entry;
    let mut current_distance = l2_distance(query, &graph.nodes[current].vector);
    loop {
        let mut improved = false;
        for &neighbor in &graph.nodes[current].neighbors[layer as usize] {
            let distance = l2_distance(query, &graph.nodes[neighbor].vector);
            if distance < current_distance {
                current = neighbor;
                current_distance = distance;
                improved = true;
            }
        }
        if !improved {
            return current;
        }
    }
}

fn search_layer(
    graph: &HnswGraph,
    query: &[f32],
    entry_points: &[usize],
    ef: usize,
    layer: u8,
) -> Vec<usize> {
    if ef == 0 {
        return Vec::new();
    }
    let mut visited = HashSet::new();
    let mut candidates: Vec<(usize, f32)> = entry_points
        .iter()
        .copied()
        .filter(|node_id| *node_id < graph.nodes.len())
        .map(|node_id| (node_id, l2_distance(query, &graph.nodes[node_id].vector)))
        .collect();
    for (node_id, _) in &candidates {
        visited.insert(*node_id);
    }
    let mut best = candidates.clone();

    while let Some((candidate_id, candidate_distance)) = pop_nearest(&mut candidates) {
        let worst_best = best
            .iter()
            .map(|(_, distance)| *distance)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(f32::INFINITY);
        if best.len() >= ef && candidate_distance > worst_best {
            break;
        }
        for &neighbor in &graph.nodes[candidate_id].neighbors[layer as usize] {
            if visited.contains(&neighbor) {
                continue;
            }
            visited.insert(neighbor);
            let distance = l2_distance(query, &graph.nodes[neighbor].vector);
            if best.len() < ef || distance < worst_best {
                candidates.push((neighbor, distance));
                best.push((neighbor, distance));
                trim_worst(&mut best, ef);
            }
        }
    }

    best.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    best.into_iter().map(|(node_id, _)| node_id).collect()
}

fn pop_nearest(candidates: &mut Vec<(usize, f32)>) -> Option<(usize, f32)> {
    if candidates.is_empty() {
        return None;
    }
    let mut best_idx = 0usize;
    for idx in 1..candidates.len() {
        if candidates[idx]
            .1
            .partial_cmp(&candidates[best_idx].1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .is_lt()
        {
            best_idx = idx;
        }
    }
    Some(candidates.swap_remove(best_idx))
}

fn trim_worst(best: &mut Vec<(usize, f32)>, limit: usize) {
    if best.len() <= limit {
        return;
    }
    best.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    best.truncate(limit);
}

fn select_best(mut nodes: Vec<usize>, limit: usize) -> Vec<usize> {
    if nodes.len() > limit {
        nodes.truncate(limit);
    }
    nodes
}

fn connect_bidirectional(
    graph: &mut HnswGraph,
    left: usize,
    right: usize,
    layer: u8,
    max_connections: usize,
) {
    if left == right {
        return;
    }
    link_one_way(graph, left, right, layer, max_connections);
    link_one_way(graph, right, left, layer, max_connections);
}

fn link_one_way(graph: &mut HnswGraph, src: usize, dst: usize, layer: u8, max_connections: usize) {
    let layer_idx = layer as usize;
    {
        let neighbors = &mut graph.nodes[src].neighbors[layer_idx];
        if neighbors.contains(&dst) {
            return;
        }
        neighbors.push(dst);
    }
    if graph.nodes[src].neighbors[layer_idx].len() > max_connections {
        let src_vector = graph.nodes[src].vector.clone();
        let mut scored: Vec<(usize, f32)> = graph.nodes[src].neighbors[layer_idx]
            .iter()
            .map(|neighbor| {
                (
                    *neighbor,
                    l2_distance(&src_vector, &graph.nodes[*neighbor].vector),
                )
            })
            .collect();
        scored.sort_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        graph.nodes[src].neighbors[layer_idx] = scored
            .into_iter()
            .take(max_connections)
            .map(|(neighbor, _)| neighbor)
            .collect();
    }
}

fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn looks_like_binary_snapshot(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == HNSW_SNAPSHOT_MAGIC
}

fn encode_binary_snapshot(on_disk: &OnDisk) -> Vec<u8> {
    let mut payload = BinaryWriter::new();
    payload.write_u64(on_disk.config.max_connections as u64);
    payload.write_u64(on_disk.config.ef_construction as u64);
    payload.write_u64(on_disk.config.ef_search as u64);
    payload.write_u8(on_disk.config.max_level);

    let mut dims: Vec<usize> = on_disk.graphs.keys().copied().collect();
    dims.sort_unstable();
    payload.write_u32(dims.len() as u32);
    for dim in dims {
        let graph = &on_disk.graphs[&dim];
        payload.write_u32(dim as u32);
        match graph.entry_point {
            Some(entry) => {
                payload.write_u8(1);
                payload.write_u32(entry as u32);
            }
            None => payload.write_u8(0),
        }
        payload.write_u8(graph.max_level);
        payload.write_u32(graph.nodes.len() as u32);
        for node in &graph.nodes {
            payload.write_u64(node.rid.0);
            payload.write_u32(node.vector.len() as u32);
            for value in &node.vector {
                payload.write_u32(value.to_bits());
            }
            payload.write_u8(node.level);
            payload.write_u32(node.neighbors.len() as u32);
            for layer_neighbors in &node.neighbors {
                payload.write_u32(layer_neighbors.len() as u32);
                for neighbor in layer_neighbors {
                    payload.write_u32(*neighbor as u32);
                }
            }
        }
    }

    let payload = payload.into_inner();
    let mut out = BinaryWriter::new();
    out.write_u32(HNSW_SNAPSHOT_MAGIC);
    out.write_u8(HNSW_SNAPSHOT_VERSION);
    out.write_u32(payload.len() as u32);
    out.write_u32(crc32c(&payload));
    out.write_bytes(&payload);
    out.into_inner()
}

fn decode_binary_snapshot(bytes: &[u8], source: &Path) -> Result<OnDisk> {
    if bytes.len() < 13 {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated HNSW snapshot header".to_string(),
        });
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != HNSW_SNAPSHOT_MAGIC {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "bad HNSW snapshot magic".to_string(),
        });
    }
    if bytes[4] != HNSW_SNAPSHOT_VERSION {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!("unsupported HNSW snapshot version {}", bytes[4]),
        });
    }
    let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    let expected_crc = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    if bytes.len() < 13 + payload_len {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated HNSW snapshot payload".to_string(),
        });
    }
    let payload = &bytes[13..13 + payload_len];
    let actual_crc = crc32c(payload);
    if actual_crc != expected_crc {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!(
                "HNSW snapshot checksum mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
            ),
        });
    }
    let mut reader = BinaryReader::new(payload, source.to_path_buf());
    let config = HnswConfig {
        max_connections: reader.read_u64()? as usize,
        ef_construction: reader.read_u64()? as usize,
        ef_search: reader.read_u64()? as usize,
        max_level: reader.read_u8()?,
    };
    let graph_count = reader.read_u32()? as usize;
    let mut graphs = HashMap::with_capacity(graph_count);
    for _ in 0..graph_count {
        let dim = reader.read_u32()? as usize;
        let entry_point = match reader.read_u8()? {
            0 => None,
            1 => Some(reader.read_u32()? as usize),
            tag => {
                return Err(TridentError::Corrupt {
                    path: source.to_path_buf(),
                    reason: format!("invalid HNSW entry-point tag {tag}"),
                });
            }
        };
        let max_level = reader.read_u8()?;
        let node_count = reader.read_u32()? as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let rid = RecordId(reader.read_u64()?);
            let vector_len = reader.read_u32()? as usize;
            let mut vector = Vec::with_capacity(vector_len);
            for _ in 0..vector_len {
                vector.push(f32::from_bits(reader.read_u32()?));
            }
            let level = reader.read_u8()?;
            let layer_count = reader.read_u32()? as usize;
            let mut neighbors = Vec::with_capacity(layer_count);
            for _ in 0..layer_count {
                let ncount = reader.read_u32()? as usize;
                let mut layer_neighbors = Vec::with_capacity(ncount);
                for _ in 0..ncount {
                    layer_neighbors.push(reader.read_u32()? as usize);
                }
                neighbors.push(layer_neighbors);
            }
            nodes.push(GraphNode {
                rid,
                vector,
                level,
                neighbors,
            });
        }
        graphs.insert(
            dim,
            HnswGraph {
                nodes,
                entry_point,
                max_level,
            },
        );
    }
    Ok(OnDisk { config, graphs })
}
