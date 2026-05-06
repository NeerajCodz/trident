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

pub mod adjacency;
pub mod btree;
pub mod hnsw;
pub mod lsm;

pub use adjacency::AdjacencyIndex;
pub use btree::BTreeIndex;
pub use hnsw::HnswIndex;
pub use lsm::LsmIndex;

use crate::errors::Result;
use crate::store::RecordId;

/// The interface every index plugin must implement.
///
/// An index plugin stores `key → RecordId` mappings only.  It never holds
/// a copy of the actual value bytes.
pub trait IndexPlugin: Send + Sync {
    /// Human-readable name used to derive on-disk file names.
    fn name(&self) -> &str;

    /// Associate `key` with `rid`.
    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()>;

    /// Look up the [`RecordId`] for `key`, or `None` if absent.
    fn get(&self, key: &[u8]) -> Option<RecordId>;

    /// Remove the mapping for `key` (insert a tombstone).
    fn delete(&mut self, key: &[u8]) -> Result<()>;

    /// Persist in-memory state to disk.
    fn flush(&mut self) -> Result<()>;
}
