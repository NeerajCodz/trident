use crate::errors::{Result, TridentError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TridentConfig {
    pub data_dir: PathBuf,
    pub page_size: usize,
    pub segment_size: usize,
    pub wal_sync_policy: WalSyncPolicy,
    pub cache_size_bytes: usize,
    pub compression: Compression,
    pub checksum: ChecksumMode,
    pub background_workers: usize,
    pub direct_io: bool,
    pub accelerator: AcceleratorBackend,
    pub large_value_threshold: usize,
    pub memtable_flush_threshold_bytes: usize,
}

impl TridentConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.page_size < 4096 {
            return Err(TridentError::InvalidConfig(
                "page_size must be at least 4096 bytes".to_string(),
            ));
        }
        if !self.page_size.is_power_of_two() {
            return Err(TridentError::InvalidConfig(
                "page_size must be a power of two".to_string(),
            ));
        }
        if self.segment_size < self.page_size {
            return Err(TridentError::InvalidConfig(
                "segment_size must be at least page_size".to_string(),
            ));
        }
        if self.cache_size_bytes < self.page_size {
            return Err(TridentError::InvalidConfig(
                "cache_size_bytes must be at least page_size".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for TridentConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(".trident"),
            page_size: 16 * 1024,
            segment_size: 64 * 1024 * 1024,
            wal_sync_policy: WalSyncPolicy::EveryBatch,
            cache_size_bytes: 256 * 1024 * 1024,
            compression: Compression::Lz4,
            checksum: ChecksumMode::Crc32c,
            background_workers: 2,
            direct_io: false,
            accelerator: AcceleratorBackend::Cpu,
            large_value_threshold: 64 * 1024,
            memtable_flush_threshold_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WalSyncPolicy {
    EveryBatch,
    GroupCommit,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Compression {
    None,
    Lz4,
    Zstd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ChecksumMode {
    None,
    Crc32c,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AcceleratorBackend {
    Cpu,
    Cuda,
    Vulkan,
    Metal,
}
