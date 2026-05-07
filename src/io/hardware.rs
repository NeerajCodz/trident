#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardwareBackend {
    BufferedIo,
    DirectIo,
    AsyncIo,
    IoUring,
    Spdk,
    Pmem,
    Rdma,
    ZnsSsd,
    Simd,
    Gpu,
    ObjectStorage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardwareOperation {
    Read,
    Write,
    Fsync,
    Checksum,
    Compression,
    BloomProbe,
    VectorDistance,
    AnnScoring,
    ColumnarFilter,
    ReplicationTransfer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardwareCapability {
    pub backend: HardwareBackend,
    pub operation: HardwareOperation,
    pub available: bool,
    pub cpu_fallback: bool,
}

impl HardwareCapability {
    pub fn portable(backend: HardwareBackend, operation: HardwareOperation) -> Self {
        Self {
            backend,
            operation,
            available: false,
            cpu_fallback: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardwareIoProfile {
    pub capabilities: Vec<HardwareCapability>,
}

impl Default for HardwareIoProfile {
    fn default() -> Self {
        Self {
            capabilities: vec![
                HardwareCapability {
                    backend: HardwareBackend::BufferedIo,
                    operation: HardwareOperation::Read,
                    available: true,
                    cpu_fallback: true,
                },
                HardwareCapability {
                    backend: HardwareBackend::BufferedIo,
                    operation: HardwareOperation::Write,
                    available: true,
                    cpu_fallback: true,
                },
                HardwareCapability::portable(HardwareBackend::IoUring, HardwareOperation::Read),
                HardwareCapability::portable(HardwareBackend::Spdk, HardwareOperation::Write),
                HardwareCapability::portable(HardwareBackend::Pmem, HardwareOperation::Fsync),
                HardwareCapability::portable(
                    HardwareBackend::Rdma,
                    HardwareOperation::ReplicationTransfer,
                ),
                HardwareCapability::portable(HardwareBackend::ZnsSsd, HardwareOperation::Write),
                HardwareCapability {
                    backend: HardwareBackend::Simd,
                    operation: HardwareOperation::Checksum,
                    available: true,
                    cpu_fallback: true,
                },
                HardwareCapability::portable(HardwareBackend::Gpu, HardwareOperation::AnnScoring),
            ],
        }
    }
}

impl HardwareIoProfile {
    pub fn supports(&self, backend: HardwareBackend, operation: HardwareOperation) -> bool {
        self.capabilities.iter().any(|capability| {
            capability.backend == backend
                && capability.operation == operation
                && capability.available
        })
    }

    pub fn has_cpu_fallback(&self, operation: HardwareOperation) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.operation == operation && capability.cpu_fallback)
    }
}
