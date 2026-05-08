pub mod invariants;
pub mod physical;

use crate::errors::Result;
use crate::store::RecordId;

pub use invariants::{
    CanonicalValuePolicy, DurableArtifactDescriptor, DurableArtifactKind, DurableArtifactState,
    DurableFormatDescriptor, KernelInvariant, KernelInvariantReport, KernelInvariantValidator,
    ManifestTracked, MemoryOwnership, RecoverableStructure, StorageOperationMetrics,
};
pub use physical::{
    AnalyticalProjection, EngineCapability, EngineRole, MaterializedLayout, PhysicalEngine,
    PhysicalEngineKind, ValueStore,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionMode {
    Individual,
    Unified,
    Specialized,
    Hybrid,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelSnapshot {
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelCompactionReport {
    pub records_retained: u64,
    pub records_dropped: u64,
    pub bytes_rewritten: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelStorageReport {
    pub live_records: u64,
    pub dead_records: u64,
    pub canonical_live_bytes: u64,
}

pub trait StorageKernel {
    fn put_record(&mut self, bytes: &[u8]) -> Result<RecordId>;
    fn get_record(&self, rid: RecordId) -> Result<Vec<u8>>;
    fn delete_record(&mut self, rid: RecordId) -> Result<()>;
    fn storage_report(&self) -> KernelStorageReport;
    fn snapshot(&self) -> KernelSnapshot;
    fn flush(&mut self) -> Result<()>;
    fn compact(&mut self) -> Result<KernelCompactionReport>;
}
