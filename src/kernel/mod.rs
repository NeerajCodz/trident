use crate::errors::Result;
use crate::store::RecordId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionMode {
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

pub trait StorageKernel {
    fn put_record(&mut self, bytes: &[u8]) -> Result<RecordId>;
    fn get_record(&self, rid: RecordId) -> Result<Vec<u8>>;
    fn delete_record(&mut self, rid: RecordId) -> Result<()>;
    fn snapshot(&self) -> KernelSnapshot;
    fn flush(&mut self) -> Result<()>;
    fn compact(&mut self) -> Result<KernelCompactionReport>;
}
