use crate::errors::Result;
use crate::index::{IndexPlugin, IndexStats};
use crate::store::RecordId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct BitmapIndex {
    name: String,
    map: BTreeMap<Vec<u8>, BTreeSet<RecordId>>,
}

impl BitmapIndex {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            map: BTreeMap::new(),
        }
    }

    pub fn contains(&self, key: &[u8], rid: RecordId) -> bool {
        self.map.get(key).is_some_and(|set| set.contains(&rid))
    }

    pub fn postings(&self, key: &[u8]) -> Vec<RecordId> {
        self.map
            .get(key)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }
}

impl IndexPlugin for BitmapIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        self.map.entry(key.to_vec()).or_default().insert(rid);
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.map.get(key).and_then(|set| set.iter().next().copied())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.map.remove(key);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn stats(&self) -> IndexStats {
        IndexStats {
            live_keys: self.map.len() as u64,
            versions: self.map.values().map(|set| set.len() as u64).sum(),
        }
    }
}
