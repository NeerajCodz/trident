use crate::document::RecordId;
use crate::errors::{PraxisError, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Default)]
pub struct GraphIndex {
    adjacency: BTreeMap<RecordId, Vec<RecordId>>,
}

impl GraphIndex {
    pub fn add_edge(&mut self, source: RecordId, target: RecordId) {
        self.adjacency.entry(source).or_default().push(target);
    }

    /// Remove a record and all edges referencing it (both outbound and inbound).
    pub fn remove_record(&mut self, id: &RecordId) {
        self.adjacency.remove(id);
        for neighbors in self.adjacency.values_mut() {
            neighbors.retain(|n| n != id);
        }
    }

    pub fn bfs(&self, start: &RecordId, max_hops: usize) -> Vec<RecordId> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([(start.clone(), 0usize)]);
        let mut output = Vec::new();
        while let Some((node, depth)) = queue.pop_front() {
            if !seen.insert(node.clone()) || depth > max_hops {
                continue;
            }
            output.push(node.clone());
            if depth == max_hops {
                continue;
            }
            for next in self.adjacency.get(&node).cloned().unwrap_or_default() {
                queue.push_back((next, depth + 1));
            }
        }
        output
    }

    pub fn shortest_path(&self, start: &RecordId, target: &RecordId) -> Result<Vec<RecordId>> {
        let mut queue = VecDeque::from([(start.clone(), vec![start.clone()])]);
        let mut seen = BTreeSet::new();
        while let Some((node, path)) = queue.pop_front() {
            if node == *target {
                return Ok(path);
            }
            if !seen.insert(node.clone()) {
                continue;
            }
            for next in self.adjacency.get(&node).cloned().unwrap_or_default() {
                let mut next_path = path.clone();
                next_path.push(next.clone());
                queue.push_back((next, next_path));
            }
        }
        Err(PraxisError::Query("path not found".into()))
    }
}
