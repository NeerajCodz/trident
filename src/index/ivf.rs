use crate::accel::gpu::kernels::squared_l2_distance;
use crate::store::RecordId;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub struct VectorEntry {
    pub rid: RecordId,
    pub vector: Vec<f32>,
}

#[derive(Debug)]
pub struct IvfFlatIndex {
    dimension: usize,
    lists: Vec<Vec<VectorEntry>>,
}

impl IvfFlatIndex {
    pub fn new(dimension: usize, list_count: usize) -> Self {
        Self {
            dimension,
            lists: vec![Vec::new(); list_count.max(1)],
        }
    }

    pub fn insert(&mut self, rid: RecordId, vector: Vec<f32>) -> bool {
        if vector.len() != self.dimension {
            return false;
        }
        let list = self.route(&vector);
        self.lists[list].push(VectorEntry { rid, vector });
        true
    }

    pub fn search_exact(&self, query: &[f32], limit: usize) -> Vec<(RecordId, f32)> {
        self.search_exact_by(query, limit, |_| true)
    }

    pub fn search_exact_filtered(
        &self,
        query: &[f32],
        limit: usize,
        allowed: &[RecordId],
    ) -> Vec<(RecordId, f32)> {
        let allowed: BTreeSet<_> = allowed.iter().copied().collect();
        self.search_exact_by(query, limit, |rid| allowed.contains(&rid))
    }

    pub fn search_exact_by(
        &self,
        query: &[f32],
        limit: usize,
        mut predicate: impl FnMut(RecordId) -> bool,
    ) -> Vec<(RecordId, f32)> {
        if query.len() != self.dimension {
            return Vec::new();
        }
        let mut scored: Vec<_> = self
            .lists
            .iter()
            .flatten()
            .filter(|entry| predicate(entry.rid))
            .filter_map(|entry| {
                squared_l2_distance(&entry.vector, query).map(|score| (entry.rid, score))
            })
            .collect();
        scored.sort_by(|a, b| a.1.total_cmp(&b.1));
        scored.truncate(limit);
        scored
    }

    pub fn len(&self) -> usize {
        self.lists.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn route(&self, vector: &[f32]) -> usize {
        let seed = vector.first().copied().unwrap_or_default().to_bits() as usize;
        seed % self.lists.len()
    }
}
