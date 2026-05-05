//! B-tree-style index: `column_value → RecordId`.
//!
//! Semantically equivalent to [`LsmIndex`][super::LsmIndex] but oriented
//! toward **ordered range scans**.  Use this index when the primary access
//! pattern is `range(lower, upper)` over sorted keys (e.g. table primary
//! keys, timestamp ranges, sorted column values).
//!
//! For point lookups the [`LsmIndex`] is equally efficient; for bulk ordered
//! iteration this index is more explicit about its semantics.

use super::IndexPlugin;
use crate::errors::Result;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct OnDisk {
    entries: Vec<(Vec<u8>, u64)>,
}

/// B-tree key-to-RID index with ordered range scan support.
///
/// Stores `key → RecordId` in a sorted [`BTreeMap`].  The actual value
/// bytes live in the [`RecordStore`][crate::store::RecordStore] and are
/// never duplicated here.
pub struct BTreeIndex {
    name: String,
    dir: PathBuf,
    /// `None` values are tombstones.
    entries: BTreeMap<Vec<u8>, Option<RecordId>>,
}

impl BTreeIndex {
    /// Open or create a B-tree index named `name` inside `dir`.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let mut entries: BTreeMap<Vec<u8>, Option<RecordId>> = BTreeMap::new();
        let path = Self::snapshot_path(&dir, &name);
        if path.exists() {
            let bytes = std::fs::read(&path)?;
            let on_disk: OnDisk = serde_json::from_slice(&bytes)?;
            for (key, rid) in on_disk.entries {
                entries.insert(key, Some(RecordId(rid)));
            }
        }

        Ok(Self { name, dir, entries })
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.btidx"))
    }

    /// Forward range scan returning live `(key, rid)` pairs in `[lower, upper)`.
    ///
    /// Both bounds are optional:
    /// * `lower = None` means unbounded lower bound (start of index).
    /// * `upper = None` means unbounded upper bound (end of index).
    pub fn range(
        &self,
        lower: Option<&[u8]>,
        upper: Option<&[u8]>,
    ) -> Vec<(Vec<u8>, RecordId)> {
        let lo = lower
            .map(|b| Bound::Included(b.to_vec()))
            .unwrap_or(Bound::Unbounded);
        let hi = upper
            .map(|b| Bound::Excluded(b.to_vec()))
            .unwrap_or(Bound::Unbounded);
        self.entries
            .range((lo, hi))
            .filter_map(|(key, rid)| rid.map(|r| (key.clone(), r)))
            .collect()
    }
}

impl IndexPlugin for BTreeIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        self.entries.insert(key.to_vec(), Some(rid));
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.entries.get(key).and_then(|opt| *opt)
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.entries.insert(key.to_vec(), None);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let entries: Vec<(Vec<u8>, u64)> = self
            .entries
            .iter()
            .filter_map(|(key, rid)| rid.map(|r| (key.clone(), r.0)))
            .collect();
        let on_disk = OnDisk { entries };
        let bytes = serde_json::to_vec(&on_disk)?;
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }
}
