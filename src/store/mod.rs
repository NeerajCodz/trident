//! Primary data store — every value is stored here exactly once.
//!
//! The caller writes raw bytes and receives a stable [`RecordId`].  All index
//! plugins store only `key → RecordId` mappings and never duplicate the actual
//! value bytes.  The [`IndirectionTable`] maps each logical `RecordId` to a
//! physical `(segment_id, record_offset, length)`, so compaction can rewrite
//! segment files and update only the indirection table; all outstanding
//! `RecordId` values remain valid.

mod engine;
mod indirection;
mod manifest;
mod record_id;
mod segment;
mod wal;

pub use engine::{
    CacheBlockKey, CacheEntryType, IndexCompactionReport, IndexInsert, MaintenanceCycleOptions,
    MaintenanceCycleReport, StorageEngine, StorageEngineStats, SuggestedMaintenanceJob,
    UnifiedBlockCache,
};
pub use indirection::{IndirectionTable, PhysicalLocation};
pub use manifest::{StorageManifest, StorageManifestStore};
pub use record_id::RecordId;
pub use segment::RecordSegment;
pub use wal::{StorageWal, StorageWalEntry, StorageWalOperation};

use crate::errors::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Statistics returned by [`RecordStore::compact`].
#[derive(Clone, Debug, Default)]
pub struct CompactionStats {
    /// Number of live records copied to the new segment.
    pub records_retained: u64,
    /// Number of dead (tombstoned) records dropped.
    pub records_dropped: u64,
    /// Total bytes of live record data written to the new segment.
    pub bytes_written: u64,
}

/// The single primary data store for Trident's no-duplication storage engine.
///
/// Raw value bytes are appended to the active segment file exactly once.
/// The returned [`RecordId`] is the canonical stable address used by all
/// index plugins; they store only `key → RecordId`, never `key → value`.
///
/// # No-duplication guarantee
///
/// If the same logical entity (a graph node, a SQL row, a vector embedding)
/// is indexed by multiple index plugins simultaneously, the value bytes
/// appear **once** on disk inside a segment file.  Each index plugin holds
/// only a 64-bit [`RecordId`] pointer.
///
/// # Compaction
///
/// Calling [`RecordStore::compact`] rewrites all live records into a fresh
/// segment, eliminates dead (tombstoned) entries, and updates the
/// [`IndirectionTable`] in-place.  All outstanding [`RecordId`] values
/// remain valid; index plugins require no changes.
pub struct RecordStore {
    dir: PathBuf,
    active_segment: RecordSegment,
    indirection: IndirectionTable,
}

impl RecordStore {
    const SEGMENT_SUBDIR: &'static str = "records";
    const INDIRECTION_FILE: &'static str = "indirection.tind";

    /// Open or create a record store rooted at `dir`.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(dir.join(Self::SEGMENT_SUBDIR))?;

        let ind_path = dir.join(Self::INDIRECTION_FILE);
        let indirection = if ind_path.exists() {
            IndirectionTable::load(&ind_path)?
        } else {
            IndirectionTable::default()
        };

        let seg_id = indirection.active_segment_id();
        let seg_path = Self::seg_path(&dir, seg_id);
        let active_segment = RecordSegment::open(seg_path, seg_id)?;

        Ok(Self {
            dir,
            active_segment,
            indirection,
        })
    }

    fn seg_path(dir: &Path, id: u32) -> PathBuf {
        dir.join(Self::SEGMENT_SUBDIR)
            .join(format!("{id:08x}.trec"))
    }

    /// Write `bytes` to the primary store and return a stable [`RecordId`].
    ///
    /// The bytes are appended to the active segment file exactly once,
    /// regardless of how many index plugins will subsequently reference
    /// this record.
    pub fn put(&mut self, bytes: &[u8]) -> Result<RecordId> {
        let (record_offset, length) = self.active_segment.append(bytes)?;
        let loc = PhysicalLocation {
            segment_id: self.active_segment.segment_id(),
            record_offset,
            length,
        };
        Ok(self.indirection.allocate(loc))
    }

    /// Retrieve the bytes associated with `rid`.
    pub fn get(&self, rid: RecordId) -> Result<Vec<u8>> {
        let loc = self.indirection.locate(rid)?;
        let path = Self::seg_path(&self.dir, loc.segment_id);
        RecordSegment::read_at(&path, loc.record_offset, loc.length)
    }

    /// Mark `rid` as deleted.  Space is reclaimed on the next [`Self::compact`].
    pub fn delete(&mut self, rid: RecordId) -> Result<()> {
        self.indirection.tombstone(rid)
    }

    /// Persist the indirection table to disk.
    ///
    /// Must be called before the store is dropped to ensure that all written
    /// records survive a restart.
    pub fn flush(&mut self) -> Result<()> {
        self.indirection
            .save(&self.dir.join(Self::INDIRECTION_FILE))
    }

    /// Garbage-collect dead records.
    ///
    /// All live records are rewritten to a fresh segment; dead ones are
    /// dropped.  The [`IndirectionTable`] is updated in-place so every
    /// outstanding [`RecordId`] remains valid.  Index plugins are
    /// **not** touched because they hold logical ids, not physical offsets.
    pub fn compact(&mut self) -> Result<CompactionStats> {
        let live: Vec<RecordId> = self.indirection.live_rids().collect();
        let mut stats = CompactionStats {
            records_dropped: self.indirection.dead_count(),
            ..Default::default()
        };

        // Collect the set of old segment ids that will be superseded.
        let old_segment_ids: HashSet<u32> = live
            .iter()
            .filter_map(|rid| self.indirection.locate(*rid).ok().map(|l| l.segment_id))
            .collect();

        let new_id = self.indirection.alloc_segment_id();
        let new_path = Self::seg_path(&self.dir, new_id);
        let mut new_seg = RecordSegment::open(&new_path, new_id)?;

        for rid in &live {
            let loc = self.indirection.locate(*rid)?;
            let old_path = Self::seg_path(&self.dir, loc.segment_id);
            let bytes = RecordSegment::read_at(&old_path, loc.record_offset, loc.length)?;
            let (record_offset, length) = new_seg.append(&bytes)?;
            let new_loc = PhysicalLocation {
                segment_id: new_id,
                record_offset,
                length,
            };
            self.indirection.relocate(*rid, new_loc);
            stats.records_retained += 1;
            stats.bytes_written += length as u64;
        }

        // Swap in the new active segment.
        self.active_segment = new_seg;

        // Remove old segment files that are now superseded.
        for old_id in old_segment_ids {
            if old_id != new_id {
                let old_path = Self::seg_path(&self.dir, old_id);
                if old_path.exists() {
                    fs::remove_file(old_path)?;
                }
            }
        }

        self.flush()?;
        Ok(stats)
    }

    /// Number of live (non-deleted) records.
    pub fn live_count(&self) -> u64 {
        self.indirection.live_count()
    }

    /// Total bytes occupied by live records on disk (data bytes only).
    pub fn live_bytes(&self) -> u64 {
        self.indirection.live_bytes()
    }
}
