use crate::accel::cpu::CpuAccelerator;
use crate::accel::gpu::device::{GpuBackendKind, GpuDevice};
use crate::accel::traits::Accelerator;
use crate::config::Compression;
use crate::errors::Result;

#[derive(Debug)]
pub struct GpuAccelerator {
    device: GpuDevice,
    fallback: CpuAccelerator,
}

impl GpuAccelerator {
    pub fn new(backend: GpuBackendKind) -> Self {
        Self {
            device: GpuDevice::detect(backend),
            fallback: CpuAccelerator,
        }
    }

    pub fn device(&self) -> &GpuDevice {
        &self.device
    }

    pub fn is_hardware_accelerated(&self) -> bool {
        self.device.hardware_available
    }
}

impl Accelerator for GpuAccelerator {
    fn name(&self) -> &'static str {
        match (self.device.backend, self.device.hardware_available) {
            (GpuBackendKind::Cuda, true) => "gpu-cuda",
            (GpuBackendKind::Cuda, false) => "gpu-cuda-cpu-fallback",
            (GpuBackendKind::Vulkan, true) => "gpu-vulkan",
            (GpuBackendKind::Vulkan, false) => "gpu-vulkan-cpu-fallback",
            (GpuBackendKind::Metal, true) => "gpu-metal",
            (GpuBackendKind::Metal, false) => "gpu-metal-cpu-fallback",
            (GpuBackendKind::Wgpu, true) => "gpu-wgpu",
            (GpuBackendKind::Wgpu, false) => "gpu-wgpu-cpu-fallback",
        }
    }

    fn crc32c(&self, bytes: &[u8]) -> u32 {
        self.fallback.crc32c(bytes)
    }

    fn compare_keys(&self, left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        self.fallback.compare_keys(left, right)
    }

    fn encode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        self.fallback.encode_block(codec, bytes)
    }

    fn decode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        self.fallback.decode_block(codec, bytes)
    }
}
