use crate::errors::{Result, TridentError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PAGE_SIZE: usize = 16 * 1024;
pub const DEFAULT_BLOCK_SIZE: usize = 16 * 1024;
pub const DEFAULT_SEGMENT_SIZE: usize = 64 * 1024 * 1024;
pub const DEFAULT_WAL_SEGMENT_SIZE: usize = 64 * 1024 * 1024;
pub const DEFAULT_CACHE_SIZE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_BACKGROUND_WORKERS: usize = 2;
pub const DEFAULT_LARGE_VALUE_THRESHOLD: usize = 64 * 1024;
pub const DEFAULT_MEMTABLE_FLUSH_THRESHOLD_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_IMMUTABLE_MEMTABLE_LIMIT: usize = 4;
pub const DEFAULT_L0_SLOWDOWN_SEGMENTS: usize = 8;
pub const DEFAULT_L0_STOP_SEGMENTS: usize = 12;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TridentConfig {
    pub data_dir: PathBuf,
    pub page_size: usize,
    pub block_size: usize,
    pub segment_size: usize,
    pub wal_segment_size: usize,
    pub wal_sync_policy: WalSyncPolicy,
    pub cache_size_bytes: usize,
    pub compression: Compression,
    pub checksum: ChecksumMode,
    pub background_workers: usize,
    pub direct_io: bool,
    pub accelerator: AcceleratorBackend,
    pub large_value_threshold: usize,
    pub memtable_flush_threshold_bytes: usize,
    pub immutable_memtable_limit: usize,
    pub l0_slowdown_segments: usize,
    pub l0_stop_segments: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PersistedEngineConfig {
    pub page_size: usize,
    pub block_size: usize,
    pub segment_size: usize,
    pub wal_segment_size: usize,
    pub wal_sync_policy: WalSyncPolicy,
    pub cache_size_bytes: usize,
    pub compression: Compression,
    pub checksum: ChecksumMode,
    pub background_workers: usize,
    pub direct_io: bool,
    pub accelerator: AcceleratorBackend,
    pub large_value_threshold: usize,
    pub memtable_flush_threshold_bytes: usize,
    pub immutable_memtable_limit: usize,
    pub l0_slowdown_segments: usize,
    pub l0_stop_segments: usize,
}

impl Default for PersistedEngineConfig {
    fn default() -> Self {
        TridentConfig::default().persisted()
    }
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
        if self.block_size < 4096 {
            return Err(TridentError::InvalidConfig(
                "block_size must be at least 4096 bytes".to_string(),
            ));
        }
        if !self.block_size.is_power_of_two() {
            return Err(TridentError::InvalidConfig(
                "block_size must be a power of two".to_string(),
            ));
        }
        if self.segment_size < self.page_size {
            return Err(TridentError::InvalidConfig(
                "segment_size must be at least page_size".to_string(),
            ));
        }
        if self.wal_segment_size < self.page_size {
            return Err(TridentError::InvalidConfig(
                "wal_segment_size must be at least page_size".to_string(),
            ));
        }
        if self.cache_size_bytes < self.page_size {
            return Err(TridentError::InvalidConfig(
                "cache_size_bytes must be at least page_size".to_string(),
            ));
        }
        if self.background_workers == 0 {
            return Err(TridentError::InvalidConfig(
                "background_workers must be greater than zero".to_string(),
            ));
        }
        if self.large_value_threshold == 0 {
            return Err(TridentError::InvalidConfig(
                "large_value_threshold must be greater than zero".to_string(),
            ));
        }
        if self.memtable_flush_threshold_bytes < self.page_size {
            return Err(TridentError::InvalidConfig(
                "memtable_flush_threshold_bytes must be at least page_size".to_string(),
            ));
        }
        if self.immutable_memtable_limit == 0 {
            return Err(TridentError::InvalidConfig(
                "immutable_memtable_limit must be greater than zero".to_string(),
            ));
        }
        if self.l0_slowdown_segments == 0 {
            return Err(TridentError::InvalidConfig(
                "l0_slowdown_segments must be greater than zero".to_string(),
            ));
        }
        if self.l0_stop_segments < self.l0_slowdown_segments {
            return Err(TridentError::InvalidConfig(
                "l0_stop_segments must be greater than or equal to l0_slowdown_segments"
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub fn persisted(&self) -> PersistedEngineConfig {
        PersistedEngineConfig {
            page_size: self.page_size,
            block_size: self.block_size,
            segment_size: self.segment_size,
            wal_segment_size: self.wal_segment_size,
            wal_sync_policy: self.wal_sync_policy,
            cache_size_bytes: self.cache_size_bytes,
            compression: self.compression,
            checksum: self.checksum,
            background_workers: self.background_workers,
            direct_io: self.direct_io,
            accelerator: self.accelerator,
            large_value_threshold: self.large_value_threshold,
            memtable_flush_threshold_bytes: self.memtable_flush_threshold_bytes,
            immutable_memtable_limit: self.immutable_memtable_limit,
            l0_slowdown_segments: self.l0_slowdown_segments,
            l0_stop_segments: self.l0_stop_segments,
        }
    }

    pub fn from_file(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let bytes = std::fs::read_to_string(&path)?;
        let config = toml::from_str::<Self>(&bytes)?;
        config.validate()?;
        Ok(config)
    }
}

impl Default for TridentConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(".trident"),
            page_size: DEFAULT_PAGE_SIZE,
            block_size: DEFAULT_BLOCK_SIZE,
            segment_size: DEFAULT_SEGMENT_SIZE,
            wal_segment_size: DEFAULT_WAL_SEGMENT_SIZE,
            wal_sync_policy: WalSyncPolicy::EveryBatch,
            cache_size_bytes: DEFAULT_CACHE_SIZE_BYTES,
            compression: Compression::Lz4,
            checksum: ChecksumMode::Crc32c,
            background_workers: DEFAULT_BACKGROUND_WORKERS,
            direct_io: false,
            accelerator: AcceleratorBackend::Cpu,
            large_value_threshold: DEFAULT_LARGE_VALUE_THRESHOLD,
            memtable_flush_threshold_bytes: DEFAULT_MEMTABLE_FLUSH_THRESHOLD_BYTES,
            immutable_memtable_limit: DEFAULT_IMMUTABLE_MEMTABLE_LIMIT,
            l0_slowdown_segments: DEFAULT_L0_SLOWDOWN_SEGMENTS,
            l0_stop_segments: DEFAULT_L0_STOP_SEGMENTS,
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
