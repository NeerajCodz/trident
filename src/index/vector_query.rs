pub use crate::catalog::schema::DistanceMetric;
use crate::document::RecordId;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Dot product of two vectors.
fn dot_product(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(l, r)| l * r).sum()
}

/// Cosine similarity between two vectors.
fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let dot = dot_product(left, right);
    let mag_a = left.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b = right.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        0.0
    } else {
        dot / (mag_a * mag_b)
    }
}

/// Euclidean distance between two vectors (returned as negative for ranking).
fn euclidean_distance(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// Compute similarity score based on the distance metric.
fn score(metric: DistanceMetric, a: &[f32], b: &[f32]) -> f32 {
    match metric {
        DistanceMetric::Cosine => cosine_similarity(a, b),
        DistanceMetric::Dot => dot_product(a, b),
        DistanceMetric::Euclidean => -euclidean_distance(a, b),
    }
}

#[derive(Debug, Clone)]
pub struct VectorIndex {
    metric: DistanceMetric,
    values: BTreeMap<RecordId, Vec<f32>>,
    hnsw_layers: Vec<Vec<RecordId>>,
    hnsw_neighbors: BTreeMap<RecordId, BTreeMap<usize, Vec<RecordId>>>,
    hnsw_threshold: usize,
    hnsw_m: usize,
    hnsw_max_level: usize,
    hnsw_entry_point: Option<RecordId>,
}

impl VectorIndex {
    pub fn new(metric: DistanceMetric) -> Self {
        Self {
            metric,
            values: BTreeMap::new(),
            hnsw_layers: Vec::new(),
            hnsw_neighbors: BTreeMap::new(),
            hnsw_threshold: 10_000,
            hnsw_m: 16,
            hnsw_max_level: 0,
            hnsw_entry_point: None,
        }
    }

    pub fn with_hnsw_threshold(mut self, threshold: usize) -> Self {
        self.hnsw_threshold = threshold;
        self
    }

    pub fn insert(&mut self, id: RecordId, vector: Vec<f32>) {
        self.values.insert(id.clone(), vector);
        if self.strategy() == VectorStrategy::Hnsw {
            self.hnsw_insert_incremental(id);
        }
    }

    pub fn search(&self, query: &[f32], top: usize) -> Vec<(RecordId, f32)> {
        match self.strategy() {
            VectorStrategy::Flat => self.flat_search(query, top),
            VectorStrategy::Hnsw => self.hnsw_search(query, top),
        }
    }

    pub fn strategy(&self) -> VectorStrategy {
        if self.values.len() < self.hnsw_threshold {
            VectorStrategy::Flat
        } else {
            VectorStrategy::Hnsw
        }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn remove(&mut self, id: &RecordId) {
        if self.values.remove(id).is_none() {
            return;
        }

        for layer in &mut self.hnsw_layers {
            layer.retain(|n| n != id);
        }
        self.hnsw_neighbors.remove(id);
        for neighbors in self.hnsw_neighbors.values_mut() {
            for layer_neighbors in neighbors.values_mut() {
                layer_neighbors.retain(|n| n != id);
            }
        }

        if self.hnsw_entry_point.as_ref() == Some(id) {
            self.hnsw_entry_point = self.values.keys().next().cloned();
            if self.hnsw_entry_point.is_none() {
                self.hnsw_max_level = 0;
            }
        }

        while self.hnsw_layers.last().is_some_and(|l| l.is_empty()) {
            self.hnsw_layers.pop();
            if self.hnsw_max_level > 0 {
                self.hnsw_max_level -= 1;
            }
        }
    }

    fn distance(&self, a: &[f32], b: &[f32]) -> f32 {
        score(self.metric, a, b)
    }

    fn flat_search(&self, query: &[f32], top: usize) -> Vec<(RecordId, f32)> {
        let mut scored: Vec<_> = self
            .values
            .iter()
            .map(|(id, values)| (id.clone(), score(self.metric, values, query)))
            .collect();
        scored.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
        scored.truncate(top);
        scored
    }

    /// Incremental HNSW insertion per the Malkov & Yashunin paper.
    fn hnsw_insert_incremental(&mut self, id: RecordId) {
        let m = self.hnsw_m;
        let ef_construction = m * 2;

        // Probabilistic layer assignment: level = floor(-ln(uniform(0,1)) * mL)
        let level = self.random_level();
        let Some(query) = self.values.get(&id).cloned() else {
            return;
        };

        // Ensure layer vectors exist
        while self.hnsw_layers.len() <= level {
            self.hnsw_layers.push(Vec::new());
        }

        // If this is the first node, just insert and return
        if self.hnsw_entry_point.is_none() {
            self.hnsw_entry_point = Some(id.clone());
            self.hnsw_max_level = level;
            for l in 0..=level {
                self.hnsw_layers[l].push(id.clone());
                self.hnsw_neighbors
                    .entry(id.clone())
                    .or_default()
                    .insert(l, Vec::new());
            }
            return;
        }

        let entry = self.hnsw_entry_point.clone().unwrap();
        let mut curr_entry = entry;

        // Phase 1: Greedy search from top layer down to level+1
        for l in (level + 1..=self.hnsw_max_level).rev() {
            let (_, nearest) = self.greedy_search(&curr_entry, &query, 1, l);
            if let Some(ep) = nearest.into_iter().next() {
                curr_entry = ep;
            }
        }

        // Phase 2: At each layer from min(level, max_level) down to 0, search with ef and connect
        let min_level = level.min(self.hnsw_max_level);
        for l in (0..=min_level).rev() {
            let candidates = self.search_layer(&curr_entry, &query, ef_construction, l);
            let neighbors = self.select_neighbors_heuristic(&query, &candidates, m);

            // Insert the node into this layer
            self.hnsw_layers[l].push(id.clone());
            self.hnsw_neighbors
                .entry(id.clone())
                .or_default()
                .insert(l, neighbors.clone());

            // Add reverse edges and prune if needed
            for neighbor in &neighbors {
                self.hnsw_neighbors
                    .entry(neighbor.clone())
                    .or_default()
                    .entry(l)
                    .or_default()
                    .push(id.clone());

                // Prune neighbor's connections if they exceed max
                let neighbor_conns = self
                    .hnsw_neighbors
                    .entry(neighbor.clone())
                    .or_default()
                    .entry(l)
                    .or_default()
                    .clone();
                if neighbor_conns.len() > m * 2 {
                    let nq = self.values.get(neighbor).cloned().unwrap_or_default();
                    let scored_conns: Vec<_> = neighbor_conns
                        .iter()
                        .filter_map(|cid| {
                            let cv = self.values.get(cid)?;
                            let dist = self.distance(&nq, cv);
                            Some((cid.clone(), dist))
                        })
                        .collect();
                    let pruned = self.select_neighbors_heuristic(&nq, &scored_conns, m);
                    self.hnsw_neighbors
                        .entry(neighbor.clone())
                        .or_default()
                        .insert(l, pruned);
                }
            }

            // Update entry points for next layer
            if !candidates.is_empty() {
                curr_entry = candidates[0].0.clone();
            }
        }

        // Update entry point if this node is at a higher level
        if level > self.hnsw_max_level {
            self.hnsw_max_level = level;
            self.hnsw_entry_point = Some(id);
        }
    }

    /// Greedy beam search: find `ef` nearest neighbors to `query` at layer `layer`
    /// starting from `entry_point`.
    fn search_layer(
        &self,
        entry_point: &RecordId,
        query: &[f32],
        ef: usize,
        layer: usize,
    ) -> Vec<(RecordId, f32)> {
        let mut visited = BTreeSet::new();
        visited.insert(entry_point.clone());

        let entry_dist = self
            .values
            .get(entry_point)
            .map(|v| self.distance(v, query))
            .unwrap_or(0.0);

        // candidates (min-heap by distance), results (max-heap by distance)
        let mut candidates: Vec<(RecordId, f32)> = vec![(entry_point.clone(), entry_dist)];
        let mut results: Vec<(RecordId, f32)> = vec![(entry_point.clone(), entry_dist)];

        while !candidates.is_empty() {
            // Get closest candidate
            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            let (c_id, c_dist) = candidates.pop().unwrap();

            // If furthest result is closer than closest candidate, stop
            results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            if let Some(furthest) = results.first()
                && c_dist > furthest.1
                && results.len() >= ef
            {
                break;
            }

            // Explore neighbors
            let neighbors = self
                .hnsw_neighbors
                .get(&c_id)
                .and_then(|layers| layers.get(&layer))
                .cloned()
                .unwrap_or_default();

            for neighbor in neighbors {
                if visited.contains(&neighbor) {
                    continue;
                }
                visited.insert(neighbor.clone());

                let n_dist = self
                    .values
                    .get(&neighbor)
                    .map(|v| self.distance(v, query))
                    .unwrap_or(f32::MAX);

                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
                if results.len() < ef || n_dist < results[0].1 {
                    candidates.push((neighbor.clone(), n_dist));
                    results.push((neighbor, n_dist));
                    if results.len() > ef {
                        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
                        results.truncate(ef);
                    }
                }
            }
        }

        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        results
    }

    /// Greedy search: find `k` nearest at a single layer (for routing between layers).
    fn greedy_search(
        &self,
        entry_point: &RecordId,
        query: &[f32],
        k: usize,
        layer: usize,
    ) -> (Vec<(RecordId, f32)>, Vec<RecordId>) {
        let results = self.search_layer(entry_point, query, k.max(1), layer);
        let ids: Vec<_> = results.iter().map(|(id, _)| id.clone()).collect();
        (results, ids)
    }

    /// Heuristic neighbor selection per the HNSW paper.
    fn select_neighbors_heuristic(
        &self,
        query: &[f32],
        candidates: &[(RecordId, f32)],
        m: usize,
    ) -> Vec<RecordId> {
        let mut sorted: Vec<_> = candidates
            .iter()
            .map(|(id, _)| {
                let dist = self
                    .values
                    .get(id)
                    .map(|v| self.distance(v, query))
                    .unwrap_or(f32::MAX);
                (id.clone(), dist)
            })
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        sorted.truncate(m);
        sorted.into_iter().map(|(id, _)| id).collect()
    }

    /// Random level assignment: level = floor(-ln(uniform) * mL)
    fn random_level(&self) -> usize {
        let ml = 1.0f64 / (self.hnsw_m as f64).ln();
        // Use a simple deterministic hash of current length as pseudo-random
        let r: f64 = ((self.values.len() as u64).wrapping_mul(6364136223846793005) >> 33) as f64
            / (1u64 << 33) as f64;
        let level = (-r.ln() * ml) as usize;
        level.min(16) // cap at 16 layers
    }

    fn hnsw_search(&self, query: &[f32], top: usize) -> Vec<(RecordId, f32)> {
        let entry = match &self.hnsw_entry_point {
            Some(ep) => ep.clone(),
            None => return self.flat_search(query, top),
        };

        let mut curr_entry = entry;

        // Greedy search from top layer down to layer 1
        for l in (1..=self.hnsw_max_level).rev() {
            let (_, nearest) = self.greedy_search(&curr_entry, query, 1, l);
            if let Some(ep) = nearest.into_iter().next() {
                curr_entry = ep;
            }
        }

        // Search layer 0 with ef = top
        let results = self.search_layer(&curr_entry, query, top.max(64), 0);
        let mut scored: Vec<_> = results.into_iter().take(top).collect();

        // Fallback to flat if we didn't get enough results
        if scored.len() < top {
            return self.flat_search(query, top);
        }

        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        scored.truncate(top);
        scored
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorStrategy {
    Flat,
    Hnsw,
}
