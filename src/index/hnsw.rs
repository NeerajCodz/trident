//! Vector approximate nearest-neighbor index — Phase 5 stub.
//!
//! A full production HNSW graph will replace this in Phase 5.  The current
//! implementation provides the **correct API contract** using a CPU
//! brute-force linear scan so that callers can be written and tested today:
//!
//! * Vectors are index **keys**, not values.  They are stored here alongside
//!   the [`RecordId`] that points to the full record body in the primary
//!   [`RecordStore`][crate::store::RecordStore].
//! * The primary data store record can be anything (document text, image
//!   metadata, etc.).  `HnswIndex` does not copy that body.
//! * Distance is L2 (Euclidean).

use crate::errors::Result;
use crate::store::RecordId;

/// Vector ANN index (Phase 5 brute-force stub).
///
/// Stores `vector → RecordId` pairs in memory and performs linear scan on
/// every search.  Replace with a real HNSW graph in Phase 5.
pub struct HnswIndex {
    name: String,
    vectors: Vec<(Vec<f32>, RecordId)>,
}

impl HnswIndex {
    /// Create a new in-memory vector index with the given `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vectors: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Index `vector` as the lookup key for `rid`.
    ///
    /// The vector is the ANN search key.  `rid` is the logical address of the
    /// associated record in the primary data store; the full record body is
    /// not stored here.
    pub fn insert(&mut self, vector: Vec<f32>, rid: RecordId) -> Result<()> {
        self.vectors.push((vector, rid));
        Ok(())
    }

    /// Return the top-`k` nearest neighbors to `query` as `(rid, distance)`,
    /// sorted by ascending L2 distance.
    ///
    /// Vectors whose length does not match `query.len()` are skipped.
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

    /// Total number of indexed vectors.
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
}

fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}
