//! Vector approximate nearest-neighbor index — Phase 5 implementation scaffold.
//!
//! Current behavior is brute-force linear scan for correctness; vectors can be
//! persisted/reloaded so storage-engine workflows can survive restarts before
//! a full HNSW graph backend is introduced.

use crate::errors::Result;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct OnDisk {
    vectors: Vec<(Vec<f32>, RecordId)>,
}

/// Vector ANN index (linear-scan backend with persistence support).
pub struct HnswIndex {
    name: String,
    dir: Option<PathBuf>,
    vectors: Vec<(Vec<f32>, RecordId)>,
}

impl HnswIndex {
    /// Create a new in-memory vector index with the given `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            dir: None,
            vectors: Vec::new(),
        }
    }

    /// Open or create a persisted vector index.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let path = snapshot_path(&dir, &name);
        let vectors = if path.exists() {
            let bytes = std::fs::read(path)?;
            let on_disk: OnDisk = serde_json::from_slice(&bytes)?;
            on_disk.vectors
        } else {
            Vec::new()
        };
        Ok(Self {
            name,
            dir: Some(dir),
            vectors,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Index `vector` as the lookup key for `rid`.
    pub fn insert(&mut self, vector: Vec<f32>, rid: RecordId) -> Result<()> {
        self.vectors.push((vector, rid));
        Ok(())
    }

    /// Return the top-`k` nearest neighbors to `query` as `(rid, distance)`,
    /// sorted by ascending L2 distance.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<(RecordId, f32)> {
        let mut scored: Vec<(RecordId, f32)> = self
            .vectors
            .iter()
            .filter(|(vec, _)| vec.len() == query.len())
            .map(|(vec, rid)| (*rid, l2_distance(query, vec)))
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Persist vectors when opened in disk-backed mode.
    pub fn flush(&mut self) -> Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        let on_disk = OnDisk {
            vectors: self.vectors.clone(),
        };
        let bytes = serde_json::to_vec(&on_disk)?;
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

fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}
