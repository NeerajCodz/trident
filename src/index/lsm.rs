//! LSM-style index: `encoded_key → RecordId`.
//!
//! The in-memory component is a sorted [`BTreeMap`].  Deletions are
//! represented as tombstones (`None`).  On [`LsmIndex::flush`] the
//! live entries are serialized to a compact on-disk snapshot; on
//! [`LsmIndex::open`] the snapshot is loaded back into memory.
//!
//! For range scans see [`LsmIndex::scan`].

use super::IndexPlugin;
use crate::errors::Result;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct OnDisk {
    /// Sorted list of (key, rid); tombstones are excluded.
    entries: Vec<(Vec<u8>, u64)>,
}

/// LSM-style key-to-RID index.
///
/// Stores `encoded_key → RecordId`.  The data itself lives in the
/// [`RecordStore`][crate::store::RecordStore]; this index stores only the
/// pointer.
pub struct LsmIndex {
    name: String,
    dir: PathBuf,
    /// `None` values are tombstones.
    memtable: BTreeMap<Vec<u8>, Option<RecordId>>,
}

impl LsmIndex {
    /// Open or create an LSM index named `name` inside `dir`.
    ///
    /// If a snapshot file `<dir>/<name>.lsm` exists it is loaded into
    /// the in-memory memtable.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let mut memtable: BTreeMap<Vec<u8>, Option<RecordId>> = BTreeMap::new();
        let snapshot = Self::snapshot_path(&dir, &name);
        if snapshot.exists() {
            let bytes = std::fs::read(&snapshot)?;
            let on_disk: OnDisk = serde_json::from_slice(&bytes)?;
            for (key, rid) in on_disk.entries {
                memtable.insert(key, Some(RecordId(rid)));
            }
        }

        Ok(Self { name, dir, memtable })
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.lsm"))
    }

    /// Forward range scan: returns all live `(key, rid)` pairs in `[start, end)`.
    ///
    /// Both `start` and `end` are optional; pass `None` for an open-ended bound.
    pub fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Vec<(Vec<u8>, RecordId)> {
        self.memtable
            .iter()
            .filter(|(key, rid)| {
                rid.is_some()
                    && start.is_none_or(|s| key.as_slice() >= s)
                    && end.is_none_or(|e| key.as_slice() < e)
            })
            .map(|(key, rid)| (key.clone(), rid.unwrap()))
            .collect()
    }
}

impl IndexPlugin for LsmIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        self.memtable.insert(key.to_vec(), Some(rid));
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.memtable.get(key).and_then(|opt| *opt)
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.memtable.insert(key.to_vec(), None);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let entries: Vec<(Vec<u8>, u64)> = self
            .memtable
            .iter()
            .filter_map(|(key, rid)| rid.map(|r| (key.clone(), r.0)))
            .collect();
        let on_disk = OnDisk { entries };
        let bytes = serde_json::to_vec(&on_disk)?;
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }
}
