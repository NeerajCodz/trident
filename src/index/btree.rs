//! B-tree-style index: `column_value → RecordId` with sequence-aware snapshots.

use super::{IndexPlugin, IndexStats};
use crate::errors::Result;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct VersionedRid {
    sequence: u64,
    rid: Option<RecordId>,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    next_sequence: u64,
    entries: Vec<(Vec<u8>, Vec<VersionedRid>)>,
    #[serde(default = "default_page_size")]
    page_size: usize,
    #[serde(default)]
    pages: Vec<BTreePageMetadata>,
}

fn default_page_size() -> usize {
    128
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BTreePageMetadata {
    pub page_id: u64,
    pub key_count: usize,
    pub min_key: Vec<u8>,
    pub max_key: Vec<u8>,
}

/// B-tree key-to-RID index with ordered range scan support.
pub struct BTreeIndex {
    name: String,
    dir: PathBuf,
    entries: BTreeMap<Vec<u8>, Vec<VersionedRid>>,
    next_sequence: u64,
    page_size: usize,
    pages: Vec<BTreePageMetadata>,
}

impl BTreeIndex {
    /// Open or create a B-tree index named `name` inside `dir`.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let mut entries = BTreeMap::new();
        let mut next_sequence = 0;
        let mut page_size = default_page_size();
        let mut pages = Vec::new();
        let path = Self::snapshot_path(&dir, &name);
        if path.exists() {
            let bytes = std::fs::read(&path)?;
            let on_disk: OnDisk = serde_json::from_slice(&bytes)?;
            next_sequence = on_disk.next_sequence;
            entries.extend(on_disk.entries);
            page_size = on_disk.page_size;
            pages = on_disk.pages;
        }

        let mut index = Self {
            name,
            dir,
            entries,
            next_sequence,
            page_size,
            pages,
        };
        index.refresh_pages();
        Ok(index)
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.btidx"))
    }

    /// Forward range scan returning live `(key, rid)` pairs in `[lower, upper)`.
    pub fn range(&self, lower: Option<&[u8]>, upper: Option<&[u8]>) -> Vec<(Vec<u8>, RecordId)> {
        let lo = lower
            .map(|b| Bound::Included(b.to_vec()))
            .unwrap_or(Bound::Unbounded);
        let hi = upper
            .map(|b| Bound::Excluded(b.to_vec()))
            .unwrap_or(Bound::Unbounded);
        self.entries
            .range((lo, hi))
            .filter_map(|(key, versions)| {
                latest_visible(versions, u64::MAX).map(|rid| (key.clone(), rid))
            })
            .collect()
    }

    fn next_seq(&mut self) -> u64 {
        self.next_sequence += 1;
        self.next_sequence
    }

    /// Deterministic page metadata generated from sorted live keys.
    pub fn page_metadata(&self) -> &[BTreePageMetadata] {
        &self.pages
    }

    fn refresh_pages(&mut self) {
        self.pages.clear();
        let live_keys: Vec<Vec<u8>> = self
            .entries
            .iter()
            .filter_map(|(key, versions)| {
                latest_visible(versions, u64::MAX)
                    .is_some()
                    .then_some(key.clone())
            })
            .collect();
        for (idx, chunk) in live_keys.chunks(self.page_size).enumerate() {
            let min_key = chunk.first().cloned().unwrap_or_default();
            let max_key = chunk.last().cloned().unwrap_or_default();
            self.pages.push(BTreePageMetadata {
                page_id: idx as u64,
                key_count: chunk.len(),
                min_key,
                max_key,
            });
        }
    }
}

impl IndexPlugin for BTreeIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        let sequence = self.next_seq();
        self.put_with_sequence(key, rid, sequence)
    }

    fn put_with_sequence(&mut self, key: &[u8], rid: RecordId, sequence: u64) -> Result<()> {
        self.next_sequence = self.next_sequence.max(sequence);
        self.entries
            .entry(key.to_vec())
            .or_default()
            .push(VersionedRid {
                sequence,
                rid: Some(rid),
            });
        self.refresh_pages();
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.get_at(key, u64::MAX)
    }

    fn get_at(&self, key: &[u8], sequence: u64) -> Option<RecordId> {
        self.entries
            .get(key)
            .and_then(|versions| latest_visible(versions, sequence))
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let sequence = self.next_seq();
        self.delete_with_sequence(key, sequence)
    }

    fn delete_with_sequence(&mut self, key: &[u8], sequence: u64) -> Result<()> {
        self.next_sequence = self.next_sequence.max(sequence);
        self.entries
            .entry(key.to_vec())
            .or_default()
            .push(VersionedRid {
                sequence,
                rid: None,
            });
        self.refresh_pages();
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let on_disk = OnDisk {
            next_sequence: self.next_sequence,
            entries: self
                .entries
                .iter()
                .map(|(key, versions)| (key.clone(), versions.clone()))
                .collect(),
            page_size: self.page_size,
            pages: self.pages.clone(),
        };
        let bytes = serde_json::to_vec(&on_disk)?;
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }

    fn compact(&mut self) -> Result<()> {
        for versions in self.entries.values_mut() {
            if let Some(last) = versions.last().copied() {
                versions.clear();
                versions.push(last);
            }
        }
        self.entries.retain(|_, versions| versions[0].rid.is_some());
        self.refresh_pages();
        Ok(())
    }

    fn stats(&self) -> IndexStats {
        let live_keys = self
            .entries
            .values()
            .filter(|versions| latest_visible(versions, u64::MAX).is_some())
            .count() as u64;
        let versions = self.entries.values().map(|v| v.len() as u64).sum();
        IndexStats {
            live_keys,
            versions,
        }
    }
}

fn latest_visible(versions: &[VersionedRid], sequence: u64) -> Option<RecordId> {
    versions
        .iter()
        .rev()
        .find(|version| version.sequence <= sequence)
        .and_then(|version| version.rid)
}
