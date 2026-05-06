//! Trident: Production Storage Engine with Single-Copy Guarantee
//!
//! Every piece of data is stored exactly once in the primary data store.
//! All indexes (LSM, B-tree, Adjacency, HNSW) hold only pointer-to-RecordId mappings.

pub mod cache;
pub mod cli;
pub mod config;
pub mod disk;
pub mod engine;
pub mod errors;
pub mod io;
pub mod maintenance;
pub mod manifest;
pub mod metrics;
pub mod recovery;
pub mod segments;
pub mod slog;
pub mod store;
pub mod transactions;
pub mod types;
pub mod values;
pub mod wal;

pub mod accel;
pub mod bench;
pub mod bench_advanced;
pub mod ram;
pub mod server;

pub use engine::TridentEngine;
pub use errors::{Result, TridentError};
pub use store::RecordId;
