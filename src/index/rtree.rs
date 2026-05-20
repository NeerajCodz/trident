use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;
use std::cmp::Ordering;

const MAX_ENTRIES: usize = 16;
const MIN_ENTRIES: usize = 4;

/// Axis-aligned bounding box for geospatial indexing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BoundingBox {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self { min_x, min_y, max_x, max_y }
    }

    pub fn point(x: f64, y: f64) -> Self {
        Self { min_x: x, min_y: y, max_x: x, max_y: y }
    }

    pub fn intersects(self, other: Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    pub fn contains(self, other: Self) -> bool {
        self.min_x <= other.min_x
            && self.max_x >= other.max_x
            && self.min_y <= other.min_y
            && self.max_y >= other.max_y
    }

    pub fn contains_point(self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    pub fn area(self) -> f64 {
        (self.max_x - self.min_x) * (self.max_y - self.min_y)
    }

    pub fn merge(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    pub fn distance_to_point(self, x: f64, y: f64) -> f64 {
        let dx = if x < self.min_x {
            self.min_x - x
        } else if x > self.max_x {
            x - self.max_x
        } else {
            0.0
        };
        let dy = if y < self.min_y {
            self.min_y - y
        } else if y > self.max_y {
            y - self.max_y
        } else {
            0.0
        };
        (dx * dx + dy * dy).sqrt()
    }
}

/// R-tree node - either a leaf entry or an internal node.
#[derive(Debug)]
enum RTreeNode {
    Leaf {
        bounds: BoundingBox,
        rid: RecordId,
    },
    Internal {
        bounds: BoundingBox,
        children: Vec<RTreeNode>,
    },
}

impl RTreeNode {
    fn bounds(&self) -> BoundingBox {
        match self {
            RTreeNode::Leaf { bounds, .. } => *bounds,
            RTreeNode::Internal { bounds, .. } => *bounds,
        }
    }

    fn is_leaf(&self) -> bool {
        matches!(self, RTreeNode::Leaf { .. })
    }
}

/// R-tree spatial index for geospatial queries.
///
/// Supports:
/// - Rectangle intersection queries
/// - Point containment queries
/// - Nearest neighbor queries
/// - K-nearest neighbor queries
#[derive(Debug)]
pub struct PackedRTreeIndex {
    root: Option<RTreeNode>,
    size: usize,
}

impl Default for PackedRTreeIndex {
    fn default() -> Self {
        Self { root: None, size: 0 }
    }
}

impl PackedRTreeIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Insert a bounding box with its record ID.
    pub fn insert(&mut self, bounds: BoundingBox, rid: RecordId) {
        let leaf = RTreeNode::Leaf { bounds, rid };
        self.root = Some(match self.root.take() {
            None => leaf,
            Some(root) => self.insert_into(root, leaf),
        });
        self.size += 1;
    }

    /// Search for all entries whose bounding boxes intersect the query.
    pub fn search(&self, query: BoundingBox) -> Vec<RecordId> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            self.search_recursive(root, query, &mut results);
        }
        results
    }

    /// Find all entries containing a specific point.
    pub fn search_point(&self, x: f64, y: f64) -> Vec<RecordId> {
        self.search(BoundingBox::point(x, y))
    }

    /// Find the K nearest neighbors to a point.
    pub fn knn(&self, x: f64, y: f64, k: usize) -> Vec<(RecordId, f64)> {
        let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::new();
        if let Some(ref root) = self.root {
            self.knn_recursive(root, x, y, k, &mut heap);
        }
        let mut results: Vec<_> = heap.into_iter().map(|e| (e.rid, e.distance)).collect();
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        results.truncate(k);
        results
    }

    /// Find all entries within a distance from a point.
    pub fn within_distance(&self, x: f64, y: f64, radius: f64) -> Vec<(RecordId, f64)> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            self.within_distance_recursive(root, x, y, radius, &mut results);
        }
        results
    }

    fn insert_into(&mut self, root: RTreeNode, leaf: RTreeNode) -> RTreeNode {
        match root {
            RTreeNode::Internal { bounds: _, mut children } => {
                // Find best child to insert into
                let best_idx = self.choose_subtree(&children, leaf.bounds());
                let best_child = children.remove(best_idx);
                let updated = self.insert_into(best_child, leaf);
                children.push(updated);

                // Split if needed
                if children.len() > MAX_ENTRIES {
                    let (group1, group2) = self.split_children(children);
                    let bounds1 = Self::compute_bounds_group(&group1);
                    let bounds2 = Self::compute_bounds_group(&group2);
                    let new_root = RTreeNode::Internal {
                        bounds: bounds1.merge(bounds2),
                        children: vec![
                            RTreeNode::Internal { bounds: bounds1, children: group1 },
                            RTreeNode::Internal { bounds: bounds2, children: group2 },
                        ],
                    };
                    new_root
                } else {
                    let new_bounds = Self::compute_bounds_group(&children);
                    RTreeNode::Internal { bounds: new_bounds, children }
                }
            }
            RTreeNode::Leaf { .. } => {
                // Can't insert into a leaf directly - create an internal node
                let merged_bounds = root.bounds().merge(leaf.bounds());
                RTreeNode::Internal {
                    bounds: merged_bounds,
                    children: vec![root, leaf],
                }
            }
        }
    }

    fn choose_subtree(&self, children: &[RTreeNode], bounds: BoundingBox) -> usize {
        children
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let enlargement_a = a.bounds().merge(bounds).area() - a.bounds().area();
                let enlargement_b = b.bounds().merge(bounds).area() - b.bounds().area();
                enlargement_a
                    .partial_cmp(&enlargement_b)
                    .unwrap_or(Ordering::Equal)
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn split_children(&self, children: Vec<RTreeNode>) -> (Vec<RTreeNode>, Vec<RTreeNode>) {
        // Quadratic split: find pair with maximum separation
        let mut max_waste = 0.0;
        let (mut seed1, mut seed2) = (0, 1);
        for i in 0..children.len() {
            for j in (i + 1)..children.len() {
                let merged = children[i].bounds().merge(children[j].bounds());
                let waste = merged.area() - children[i].bounds().area() - children[j].bounds().area();
                if waste > max_waste {
                    max_waste = waste;
                    seed1 = i;
                    seed2 = j;
                }
            }
        }

        let mut group1 = Vec::new();
        let mut group2 = Vec::new();
        for (i, entry) in children.into_iter().enumerate() {
            if i == seed1 {
                group1.push(entry);
            } else if i == seed2 {
                group2.push(entry);
            } else {
                let bounds1 = Self::compute_bounds_group(&group1);
                let bounds2 = Self::compute_bounds_group(&group2);
                let cost1 = bounds1.merge(entry.bounds()).area() - bounds1.area();
                let cost2 = bounds2.merge(entry.bounds()).area() - bounds2.area();

                if cost1 < cost2 {
                    group1.push(entry);
                } else {
                    group2.push(entry);
                }
            }
        }

        (group1, group2)
    }

    fn compute_bounds_group(children: &[RTreeNode]) -> BoundingBox {
        children
            .iter()
            .map(|c| c.bounds())
            .reduce(|a, b| a.merge(b))
            .unwrap_or(BoundingBox::new(0.0, 0.0, 0.0, 0.0))
    }

    fn search_recursive(&self, node: &RTreeNode, query: BoundingBox, results: &mut Vec<RecordId>) {
        if !node.bounds().intersects(query) {
            return;
        }
        match node {
            RTreeNode::Leaf { rid, .. } => results.push(*rid),
            RTreeNode::Internal { children, .. } => {
                for child in children {
                    self.search_recursive(child, query, results);
                }
            }
        }
    }

    fn knn_recursive(
        &self,
        node: &RTreeNode,
        x: f64,
        y: f64,
        k: usize,
        heap: &mut BinaryHeap<HeapEntry>,
    ) {
        match node {
            RTreeNode::Leaf { bounds, rid } => {
                let dist = bounds.distance_to_point(x, y);
                if heap.len() < k {
                    heap.push(HeapEntry { rid: *rid, distance: dist });
                } else if dist < heap.peek().unwrap().distance {
                    heap.pop();
                    heap.push(HeapEntry { rid: *rid, distance: dist });
                }
            }
            RTreeNode::Internal { children, .. } => {
                // Sort children by distance to point for best-first search
                let mut sorted: Vec<_> = children.iter().collect();
                sorted.sort_by(|a, b| {
                    a.bounds()
                        .distance_to_point(x, y)
                        .partial_cmp(&b.bounds().distance_to_point(x, y))
                        .unwrap_or(Ordering::Equal)
                });
                for child in sorted {
                    let min_dist = child.bounds().distance_to_point(x, y);
                    if heap.len() < k || min_dist < heap.peek().unwrap().distance {
                        self.knn_recursive(child, x, y, k, heap);
                    }
                }
            }
        }
    }

    fn within_distance_recursive(
        &self,
        node: &RTreeNode,
        x: f64,
        y: f64,
        radius: f64,
        results: &mut Vec<(RecordId, f64)>,
    ) {
        if node.bounds().distance_to_point(x, y) > radius {
            return;
        }
        match node {
            RTreeNode::Leaf { bounds, rid } => {
                let dist = bounds.distance_to_point(x, y);
                if dist <= radius {
                    results.push((*rid, dist));
                }
            }
            RTreeNode::Internal { children, .. } => {
                for child in children {
                    self.within_distance_recursive(child, x, y, radius, results);
                }
            }
        }
    }
}

#[derive(Debug)]
struct HeapEntry {
    rid: RecordId,
    distance: f64,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Reverse for min-heap behavior
        other.distance.partial_cmp(&self.distance)
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}
