//! LSM-style index: `encoded_key → RecordId`.
//!
//! This implementation stores versioned key histories in memory and on disk:
//! each key has an ordered sequence of updates (`put` or tombstone delete).
//! Snapshot reads resolve the newest update whose sequence is `<= snapshot`.

use super::{IndexPlugin, IndexStats};
use crate::errors::Result;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
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
    #[serde(default)]
    bloom_keys: Vec<Vec<u8>>,
    #[serde(default)]
    fence_min: Option<Vec<u8>>,
    #[serde(default)]
    fence_max: Option<Vec<u8>>,
}

/// LSM-style key-to-RID index with sequence-aware snapshot lookups.
pub struct LsmIndex {
    name: String,
    dir: PathBuf,
    history: BTreeMap<Vec<u8>, Vec<VersionedRid>>,
    next_sequence: u64,
    bloom_keys: HashSet<Vec<u8>>,
    fence_min: Option<Vec<u8>>,
    fence_max: Option<Vec<u8>>,
}

impl LsmIndex {
    /// Open or create an LSM index named `name` inside `dir`.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let mut history: BTreeMap<Vec<u8>, Vec<VersionedRid>> = BTreeMap::new();
        let mut next_sequence = 0;
        let snapshot = Self::snapshot_path(&dir, &name);
        if snapshot.exists() {
            let bytes = std::fs::read(&snapshot)?;
            let on_disk: OnDisk = serde_json::from_slice(&bytes)?;
            next_sequence = on_disk.next_sequence;
            history.extend(on_disk.entries);
            let mut index = Self {
                name,
                dir,
                history,
                next_sequence,
                bloom_keys: on_disk.bloom_keys.into_iter().collect(),
                fence_min: on_disk.fence_min,
                fence_max: on_disk.fence_max,
            };
            index.refresh_metadata();
            return Ok(index);
        }

        let mut index = Self {
            name,
            dir,
            history,
            next_sequence,
            bloom_keys: HashSet::new(),
            fence_min: None,
            fence_max: None,
        };
        index.refresh_metadata();
        Ok(index)
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.lsm"))
    }

    /// Forward range scan: returns all live `(key, rid)` pairs in `[start, end)`.
    pub fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Vec<(Vec<u8>, RecordId)> {
        self.history
            .iter()
            .filter(|(key, _)| {
                start.is_none_or(|s| key.as_slice() >= s) && end.is_none_or(|e| key.as_slice() < e)
            })
            .filter_map(|(key, versions)| {
                latest_visible(versions, u64::MAX).map(|rid| (key.clone(), rid))
            })
            .collect()
    }

    fn next_seq(&mut self) -> u64 {
        self.next_sequence += 1;
        self.next_sequence
    }

    /// Approximate bloom-filter membership check (exact in this scaffold).
    pub fn may_contain_key(&self, key: &[u8]) -> bool {
        self.bloom_keys.contains(key)
    }

    /// Fence pointer bounds `(min_key, max_key)` for live keys.
    pub fn fence_bounds(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        (self.fence_min.clone(), self.fence_max.clone())
    }

    fn refresh_metadata(&mut self) {
        self.bloom_keys.clear();
        self.fence_min = None;
        self.fence_max = None;
        for (key, versions) in &self.history {
            if latest_visible(versions, u64::MAX).is_some() {
                self.bloom_keys.insert(key.clone());
                if self.fence_min.as_ref().is_none_or(|min| key < min) {
                    self.fence_min = Some(key.clone());
                }
                if self.fence_max.as_ref().is_none_or(|max| key > max) {
                    self.fence_max = Some(key.clone());
                }
            }
        }
    }
}

impl IndexPlugin for LsmIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        let sequence = self.next_seq();
        self.put_with_sequence(key, rid, sequence)
    }

    fn put_with_sequence(&mut self, key: &[u8], rid: RecordId, sequence: u64) -> Result<()> {
        self.next_sequence = self.next_sequence.max(sequence);
        self.history
            .entry(key.to_vec())
            .or_default()
            .push(VersionedRid {
                sequence,
                rid: Some(rid),
            });
        self.refresh_metadata();
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.get_at(key, u64::MAX)
    }

    fn get_at(&self, key: &[u8], sequence: u64) -> Option<RecordId> {
        self.history
            .get(key)
            .and_then(|versions| latest_visible(versions, sequence))
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let sequence = self.next_seq();
        self.delete_with_sequence(key, sequence)
    }

    fn delete_with_sequence(&mut self, key: &[u8], sequence: u64) -> Result<()> {
        self.next_sequence = self.next_sequence.max(sequence);
        self.history
            .entry(key.to_vec())
            .or_default()
            .push(VersionedRid {
                sequence,
                rid: None,
            });
        self.refresh_metadata();
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let on_disk = OnDisk {
            next_sequence: self.next_sequence,
            entries: self
                .history
                .iter()
                .map(|(key, versions)| (key.clone(), versions.clone()))
                .collect(),
            bloom_keys: self.bloom_keys.iter().cloned().collect(),
            fence_min: self.fence_min.clone(),
            fence_max: self.fence_max.clone(),
        };
        let bytes = serde_json::to_vec(&on_disk)?;
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }

    fn compact(&mut self) -> Result<()> {
        for versions in self.history.values_mut() {
            if let Some(last) = versions.last().copied() {
                versions.clear();
                versions.push(last);
            }
        }
        self.history.retain(|_, versions| versions[0].rid.is_some());
        self.refresh_metadata();
        Ok(())
    }

    fn stats(&self) -> IndexStats {
        let live_keys = self
            .history
            .values()
            .filter(|versions| latest_visible(versions, u64::MAX).is_some())
            .count() as u64;
        let versions = self.history.values().map(|v| v.len() as u64).sum();
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
