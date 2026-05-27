//! Binary format implementations for praxis index plugins
//!
//! Each index plugin (LSM, B-tree, Adjacency, HNSW) has a dedicated binary format:
//! - Standardized header (32 bytes) with magic "TRID" and CRC32 checksum
//! - Plugin-specific metadata structure
//! - Corruption detection on every read
//! - Forward-compatible version scheme

pub mod adjacency;
pub mod btree;
pub mod hnsw;
pub mod lsm;

pub use adjacency::{AdjacencyBinaryMetadata, AdjacencyBinaryReader, AdjacencyBinaryWriter, Edge};
pub use btree::{BtreeBinaryMetadata, BtreeBinaryReader, BtreeBinaryWriter};
pub use hnsw::{HnswBinaryMetadata, HnswBinaryReader, HnswBinaryWriter, VectorNode};
pub use lsm::{LsmBinaryMetadata, LsmBinaryReader, LsmBinaryWriter};
