//! Trident: Production Storage Engine with Single-Copy Guarantee
//!
//! Every piece of data is stored exactly once in the primary data store.
//! All indexes (LSM, B-tree, Adjacency, HNSW) hold only pointer-to-RecordId mappings.

pub mod accel;
pub mod api;
pub mod bench;
pub mod bench_advanced;
pub mod cache;
pub mod cli;
pub mod config;
pub mod disk;
pub mod engine;
pub mod errors;
pub mod formats;
pub mod index;
pub mod io;
pub mod maintenance;
pub mod manifest;
pub mod memory;
pub mod metrics;
pub mod recovery;
pub mod segments;
pub mod server;
pub mod slog;
pub mod storage;
pub mod store;
pub mod transactions;
pub mod types;
pub mod values;
pub mod wal;

pub use engine::TridentEngine;
pub use engine::r#async::AsyncTridentEngine;
pub use errors::{Result, TridentError};
pub use store::RecordId;
pub use types::ColumnFamily;
pub use config::{TridentConfig, Compression, CompactionStrategy, WalSyncPolicy};
pub use transactions::WriteBatch;
pub use maintenance::{JobPriority, MaintenanceRuntimeConfig, RuntimeLaneConfig};
pub use storage::lsm::LsmIndex;
pub use index::{BTreeIndex, HnswIndex, AdjacencyIndex};
pub use index::IndexPlugin;
