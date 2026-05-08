//! Index plugins for Trident's no-duplication storage engine.
//!
//! Every index plugin stores only `key → RecordId` mappings.  The actual
//! value bytes live in the primary [`RecordStore`][crate::store::RecordStore]
//! and are never copied into an index.  To read a value, the caller:
//!
//! 1. Queries an index plugin for the [`RecordId`] associated with a key.
//! 2. Calls `RecordStore::get(rid)` to fetch the raw bytes.
//!
//! This guarantees that each value is stored on disk exactly once,
//! regardless of how many index plugins reference it simultaneously.

pub mod bitmap;
pub mod btree;
pub mod hnsw;
pub mod inverted;
pub mod ivf;
pub mod rtree;
pub mod specialized;
pub mod time_series;

pub use bitmap::BitmapIndex;
pub use btree::BTreeIndex;
pub use hnsw::HnswIndex;
pub use hnsw::adjacency::AdjacencyIndex;
pub use inverted::InvertedIndex;
pub use ivf::IvfFlatIndex;
pub use rtree::{BoundingBox, PackedRTreeIndex};
pub use specialized::{SpecializedEngineContract, SpecializedEngineRegistry};
pub use time_series::TimePartition;

use crate::errors::Result;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexStats {
    pub live_keys: u64,
    pub versions: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndexStorageLayout {
    pub stores_full_values: bool,
    pub stores_record_ids: bool,
    pub stores_lossy_summaries: bool,
}

impl IndexStorageLayout {
    pub const POINTER_ONLY: Self = Self {
        stores_full_values: false,
        stores_record_ids: true,
        stores_lossy_summaries: false,
    };

    pub const POINTER_WITH_LOSSY_SUMMARIES: Self = Self {
        stores_full_values: false,
        stores_record_ids: true,
        stores_lossy_summaries: true,
    };

    pub fn validate_default_kernel_policy(self, index_name: &str) -> Result<()> {
        if self.stores_full_values {
            return Err(crate::errors::TridentError::InvalidConfig(format!(
                "index {index_name} violates pointer-only kernel policy"
            )));
        }
        if !self.stores_record_ids {
            return Err(crate::errors::TridentError::InvalidConfig(format!(
                "index {index_name} must store RecordId pointers"
            )));
        }
        Ok(())
    }
}

/// The interface every index plugin must implement.
///
/// An index plugin stores `key → RecordId` mappings only.  It never holds
/// a copy of the actual value bytes.
pub trait IndexPlugin: Send + Sync {
    /// Human-readable name used to derive on-disk file names.
    fn name(&self) -> &str;

    /// Associate `key` with `rid`.
    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()>;

    /// Associate `key` with `rid` at a caller-assigned sequence number.
    ///
    /// Default behavior ignores the supplied sequence and delegates to [`Self::put`].
    fn put_with_sequence(&mut self, key: &[u8], rid: RecordId, _sequence: u64) -> Result<()> {
        self.put(key, rid)
    }

    /// Look up the [`RecordId`] for `key`, or `None` if absent.
    fn get(&self, key: &[u8]) -> Option<RecordId>;

    /// Snapshot read: return the value visible at `sequence`.
    ///
    /// Default behavior returns the latest value from [`Self::get`].
    fn get_at(&self, key: &[u8], _sequence: u64) -> Option<RecordId> {
        self.get(key)
    }

    /// Remove the mapping for `key` (insert a tombstone).
    fn delete(&mut self, key: &[u8]) -> Result<()>;

    /// Remove the mapping for `key` at a caller-assigned sequence number.
    ///
    /// Default behavior delegates to [`Self::delete`].
    fn delete_with_sequence(&mut self, key: &[u8], _sequence: u64) -> Result<()> {
        self.delete(key)
    }

    /// Persist in-memory state to disk.
    fn flush(&mut self) -> Result<()>;

    /// Optional plugin-local compaction hook.
    fn compact(&mut self) -> Result<()> {
        Ok(())
    }

    /// Return plugin-level key/version counters.
    fn stats(&self) -> IndexStats {
        IndexStats::default()
    }

    /// Durable storage layout declaration used to enforce Trident's
    /// single-copy value contract.
    fn storage_layout(&self) -> IndexStorageLayout {
        IndexStorageLayout::POINTER_ONLY
    }
}
