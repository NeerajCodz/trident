//! Trident: primitive-first universal storage kernel with a single-copy guarantee.
//!
//! Every piece of data is stored exactly once in the primary data store.
//! All indexes (LSM, B-tree, Adjacency, HNSW) hold only pointer-to-RecordId mappings.
//!
//! The stable storage-kernel surface is `config`, `errors`, `kernel`, `store`, `index`,
//! `storage`, `transactions`, `api`, `sdk`, and `slog`. Other public modules are still
//! exported for the current beta/test harness and should be treated as experimental
//! internals until the compatibility layer is retired.

pub mod accel;
pub mod api;
pub mod bench;
pub mod bench_advanced;
pub mod cache;
pub mod catalog;
pub mod cli;
pub mod config;
pub mod datatype;
pub mod disk;
pub mod engine;
pub mod errors;
pub mod formats;
pub mod identity;
pub mod index;
pub mod io;
pub mod kernel;
pub mod layout;
pub mod maintenance;
pub mod manifest;
pub mod memory;
pub mod metrics;
pub mod page;
pub mod query;
pub mod record;
pub mod recovery;
pub mod replication;
pub mod sdk;
pub mod segments;
pub mod server;
pub mod slog;
pub mod storage;
pub mod store;
pub mod transactions;
pub mod types;
pub mod values;
pub mod wal;

pub use config::{CompactionStrategy, Compression, TridentConfig, WalSyncPolicy};
pub use engine::TridentEngine;
pub use engine::r#async::AsyncTridentEngine;
pub use errors::{Result, TridentError};
pub use identity::{Aid, Cid, Did, Eid, Fid, FieldId, Pid, RelativeVid, Rid, Sid, Vid, VidContext};
pub use index::IndexPlugin;
pub use index::{
    AdjacencyIndex, BTreeIndex, BitmapIndex, BoundingBox, HnswIndex, InvertedIndex, IvfFlatIndex,
    PackedRTreeIndex, TimePartition,
};
pub use maintenance::{JobPriority, MaintenanceRuntimeConfig, RuntimeLaneConfig};
pub use storage::lsm::LsmIndex;
pub use store::RecordId;
pub use transactions::WriteBatch;
pub use types::ColumnFamily;
