use crate::catalog::schema::DistanceMetric;
use crate::document::RecordId;
use crate::index::bm25::Bm25Index;
use crate::index::graph::GraphIndex;
use crate::index::vector_query::VectorIndex;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridWeights {
    pub text: f32,
    pub vector: f32,
    pub graph: f32,
}

impl Default for HybridWeights {
    fn default() -> Self {
        Self {
            text: 0.34,
            vector: 0.33,
            graph: 0.33,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordScoringOverride {
    pub id: RecordId,
    pub weights: HybridWeights,
}

#[derive(Debug, Clone)]
pub struct HybridHit {
    pub id: RecordId,
    pub score: f32,
    pub text_score: f32,
    pub vector_score: f32,
    pub graph_score: f32,
}

#[derive(Debug, Clone)]
pub struct RetrievalEngine {
    pub text: Bm25Index,
    pub vector: VectorIndex,
    pub graph: GraphIndex,
    default_weights: HybridWeights,
    record_overrides: BTreeMap<RecordId, HybridWeights>,
}

impl RetrievalEngine {
    pub fn new(metric: DistanceMetric) -> Self {
        Self {
            text: Bm25Index::default(),
            vector: VectorIndex::new(metric),
            graph: GraphIndex::default(),
            default_weights: HybridWeights::default(),
            record_overrides: BTreeMap::new(),
        }
    }

    pub fn set_default_weights(&mut self, weights: HybridWeights) {
        self.default_weights = weights;
    }

    pub fn set_record_weights(&mut self, id: RecordId, weights: HybridWeights) {
        self.record_overrides.insert(id, weights);
    }

    pub fn hybrid_search(
        &self,
        query_text: Option<&str>,
        query_vector: Option<&[f32]>,
        graph_start: Option<&RecordId>,
        max_hops: usize,
        top: usize,
        weights: HybridWeights,
    ) -> Vec<HybridHit> {
        self.hybrid_search_with_optional_weights(
            query_text,
            query_vector,
            graph_start,
            max_hops,
            top,
            Some(weights),
        )
    }

    pub fn hybrid_search_with_defaults(
        &self,
        query_text: Option<&str>,
        query_vector: Option<&[f32]>,
        graph_start: Option<&RecordId>,
        max_hops: usize,
        top: usize,
    ) -> Vec<HybridHit> {
        self.hybrid_search_with_optional_weights(
            query_text,
            query_vector,
            graph_start,
            max_hops,
            top,
            None,
        )
    }

    fn hybrid_search_with_optional_weights(
        &self,
        query_text: Option<&str>,
        query_vector: Option<&[f32]>,
        graph_start: Option<&RecordId>,
        max_hops: usize,
        top: usize,
        query_weights: Option<HybridWeights>,
    ) -> Vec<HybridHit> {
        let mut scores: BTreeMap<RecordId, HybridHit> = BTreeMap::new();
        if let Some(query_text) = query_text {
            for (id, score) in self.text.search(query_text) {
                let hit = scores.entry(id.clone()).or_insert_with(|| empty_hit(id));
                hit.text_score = score;
            }
        }
        if let Some(query_vector) = query_vector {
            for (id, score) in self.vector.search(query_vector, top.max(100)) {
                let hit = scores.entry(id.clone()).or_insert_with(|| empty_hit(id));
                hit.vector_score = score;
            }
        }
        if let Some(start) = graph_start {
            for (depth, id) in self.graph.bfs(start, max_hops).into_iter().enumerate() {
                let hit = scores.entry(id.clone()).or_insert_with(|| empty_hit(id));
                hit.graph_score = 1.0 / (depth as f32 + 1.0);
            }
        }
        let mut hits: Vec<_> = scores
            .into_values()
            .map(|mut hit| {
                let weights = self
                    .record_overrides
                    .get(&hit.id)
                    .copied()
                    .or(query_weights)
                    .unwrap_or(self.default_weights);
                hit.score = hit.text_score * weights.text
                    + hit.vector_score * weights.vector
                    + hit.graph_score * weights.graph;
                hit
            })
            .collect();
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
        });
        hits.truncate(top);
        hits
    }
}

fn empty_hit(id: RecordId) -> HybridHit {
    HybridHit {
        id,
        score: 0.0,
        text_score: 0.0,
        vector_score: 0.0,
        graph_score: 0.0,
    }
}
