use crate::document::RecordId;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

#[derive(Debug, Clone, Default)]
pub struct ScalarIndex {
    values: BTreeMap<String, BTreeSet<RecordId>>,
}

impl ScalarIndex {
    pub fn insert(&mut self, value: impl Into<String>, id: RecordId) {
        self.values.entry(value.into()).or_default().insert(id);
    }

    pub fn remove(&mut self, value: &str, id: &RecordId) {
        if let Some(ids) = self.values.get_mut(value) {
            ids.remove(id);
            if ids.is_empty() {
                self.values.remove(value);
            }
        }
    }

    pub fn equals(&self, value: &str) -> Vec<RecordId> {
        self.values
            .get(value)
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Range query: returns all record IDs where the indexed value is in [low, high].
    pub fn range(&self, low: &str, high: &str, low_inclusive: bool, high_inclusive: bool) -> Vec<RecordId> {
        let low_bound = if low_inclusive {
            Bound::Included(low.to_string())
        } else {
            Bound::Excluded(low.to_string())
        };
        let high_bound = if high_inclusive {
            Bound::Included(high.to_string())
        } else {
            Bound::Excluded(high.to_string())
        };
        self.values
            .range((low_bound, high_bound))
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }

    /// Greater than: value > bound
    pub fn greater_than(&self, bound: &str) -> Vec<RecordId> {
        self.values
            .range((Bound::Excluded(bound.to_string()), Bound::Unbounded))
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }

    /// Greater than or equal: value >= bound
    pub fn greater_than_or_equal(&self, bound: &str) -> Vec<RecordId> {
        self.values
            .range((Bound::Included(bound.to_string()), Bound::Unbounded))
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }

    /// Less than: value < bound
    pub fn less_than(&self, bound: &str) -> Vec<RecordId> {
        self.values
            .range((Bound::Unbounded, Bound::Excluded(bound.to_string())))
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }

    /// Less than or equal: value <= bound
    pub fn less_than_or_equal(&self, bound: &str) -> Vec<RecordId> {
        self.values
            .range((Bound::Unbounded, Bound::Included(bound.to_string())))
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.values.values().map(BTreeSet::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
