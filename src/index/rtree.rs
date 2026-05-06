use crate::store::RecordId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BoundingBox {
    pub fn intersects(self, other: Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }
}

#[derive(Debug, Default)]
pub struct PackedRTreeIndex {
    entries: Vec<(BoundingBox, RecordId)>,
}

impl PackedRTreeIndex {
    pub fn insert(&mut self, bounds: BoundingBox, rid: RecordId) {
        self.entries.push((bounds, rid));
    }

    pub fn search(&self, query: BoundingBox) -> Vec<RecordId> {
        self.entries
            .iter()
            .filter_map(|(bounds, rid)| bounds.intersects(query).then_some(*rid))
            .collect()
    }
}
