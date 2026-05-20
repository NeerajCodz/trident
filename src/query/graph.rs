use crate::index::adjacency::AdjacencyIndex;
use crate::store::RecordId;
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphTraversal {
    pub start: RecordId,
    pub max_depth: u8,
    pub edge_label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphTraversalHit {
    pub record_id: RecordId,
    pub depth: u8,
}

#[derive(Debug)]
pub struct GraphTraversalCursor {
    max_depth: u8,
    edge_label: Option<Vec<u8>>,
    queue: VecDeque<GraphTraversalHit>,
    visited: BTreeSet<RecordId>,
}

impl GraphTraversalCursor {
    pub fn new(request: &GraphTraversal) -> Self {
        let mut queue = VecDeque::new();
        queue.push_back(GraphTraversalHit {
            record_id: request.start,
            depth: 0,
        });

        Self {
            max_depth: request.max_depth,
            edge_label: request
                .edge_label
                .as_ref()
                .map(|label| label.as_bytes().to_vec()),
            queue,
            visited: BTreeSet::new(),
        }
    }

    pub fn next(&mut self, index: &AdjacencyIndex) -> Option<GraphTraversalHit> {
        while let Some(hit) = self.queue.pop_front() {
            if !self.visited.insert(hit.record_id) {
                continue;
            }

            if hit.depth < self.max_depth {
                let neighbors = match &self.edge_label {
                    Some(label) => index.neighbors_with_label(hit.record_id, label),
                    None => index
                        .neighbors(hit.record_id)
                        .into_iter()
                        .map(|edge| edge.to)
                        .collect(),
                };

                for record_id in neighbors {
                    if !self.visited.contains(&record_id) {
                        self.queue.push_back(GraphTraversalHit {
                            record_id,
                            depth: hit.depth + 1,
                        });
                    }
                }
            }

            return Some(hit);
        }

        None
    }

    pub fn collect(mut self, index: &AdjacencyIndex) -> Vec<GraphTraversalHit> {
        let mut hits = Vec::new();
        while let Some(hit) = self.next(index) {
            hits.push(hit);
        }
        hits
    }
}

pub fn traverse(index: &AdjacencyIndex, request: &GraphTraversal) -> Vec<GraphTraversalHit> {
    GraphTraversalCursor::new(request).collect(index)
}
