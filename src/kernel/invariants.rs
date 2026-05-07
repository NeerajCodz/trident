use crate::errors::{Result, TridentError};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelInvariant {
    CanonicalValuesSingleCopy,
    PointerOrientedIndexes,
    WalBeforeVisibility,
    DurableStructuresVersionedChecksummedRecoverable,
    ReadsDoNotBlockWrites,
    DeterministicCrashRecovery,
    ForwardCompatibleFormats,
    SnapshotSafeCompaction,
    ExplicitMemoryOwnership,
    ManifestTrackedDurability,
    MeasurableStorageOperations,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalValuePolicy {
    pub canonical_values_single_copy: bool,
    pub allow_replicas: bool,
    pub allow_snapshots: bool,
    pub allow_backups: bool,
    pub allow_erasure_redundancy: bool,
    pub allow_lossy_summaries: bool,
}

impl CanonicalValuePolicy {
    pub const STRICT: Self = Self {
        canonical_values_single_copy: true,
        allow_replicas: true,
        allow_snapshots: true,
        allow_backups: true,
        allow_erasure_redundancy: true,
        allow_lossy_summaries: true,
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableFormatDescriptor {
    pub structure: &'static str,
    pub format_version: u16,
    pub checksum: &'static str,
    pub forward_compatible: bool,
    pub independently_recoverable: bool,
}

impl DurableFormatDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.format_version == 0 {
            return Err(TridentError::InvalidConfig(format!(
                "{} has no durable format version",
                self.structure
            )));
        }
        if self.checksum.is_empty() {
            return Err(TridentError::InvalidConfig(format!(
                "{} has no durable checksum",
                self.structure
            )));
        }
        if !self.independently_recoverable {
            return Err(TridentError::InvalidConfig(format!(
                "{} is not independently recoverable",
                self.structure
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StorageOperationMetrics {
    pub latency_us: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub io_ops: u64,
    pub memory_bytes: u64,
    pub cpu_units: u64,
    pub read_amplification: f64,
    pub write_amplification: f64,
    pub space_amplification: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryOwnership {
    Arena,
    Slab,
    Epoch,
    RefCount,
    PageLifetime,
    Borrowed,
    Owned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestTracked {
    pub logical_name: String,
    pub path: PathBuf,
    pub format_version: u16,
}

impl ManifestTracked {
    pub fn new(
        logical_name: impl Into<String>,
        path: impl Into<PathBuf>,
        format_version: u16,
    ) -> Self {
        Self {
            logical_name: logical_name.into(),
            path: path.into(),
            format_version,
        }
    }
}

pub trait RecoverableStructure {
    fn durable_format(&self) -> DurableFormatDescriptor;
    fn manifest_path(&self) -> &Path;

    fn validate_recovery_contract(&self) -> Result<()> {
        self.durable_format().validate()?;
        if self.manifest_path().as_os_str().is_empty() {
            return Err(TridentError::InvalidConfig(
                "recoverable structure has no manifest path".to_string(),
            ));
        }
        Ok(())
    }
}
