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

    fn squared_l2_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        match self.device.backend {
            GpuBackendKind::Cuda => crate::accel::gpu::cuda::vector::squared_l2_distance(left, right),
            GpuBackendKind::Vulkan => crate::accel::gpu::vulkan::compute::squared_l2_distance(left, right),
            GpuBackendKind::Metal => crate::accel::gpu::metal::compute::squared_l2_distance(left, right),
            GpuBackendKind::Wgpu => crate::accel::gpu::wgpu::compute::squared_l2_distance(left, right),
        }
    }

    fn inner_product_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        match self.device.backend {
            GpuBackendKind::Cuda => crate::accel::gpu::cuda::vector::inner_product(left, right),
            GpuBackendKind::Vulkan => crate::accel::gpu::vulkan::compute::inner_product(left, right),
            GpuBackendKind::Metal => crate::accel::gpu::metal::compute::inner_product(left, right),
            GpuBackendKind::Wgpu => crate::accel::gpu::wgpu::compute::inner_product(left, right),
        }
    }

    fn cosine_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        match self.device.backend {
            GpuBackendKind::Cuda => crate::accel::gpu::cuda::vector::cosine_distance(left, right),
            GpuBackendKind::Vulkan => crate::accel::gpu::vulkan::compute::cosine_distance(left, right),
            GpuBackendKind::Metal => crate::accel::gpu::metal::compute::cosine_distance(left, right),
            GpuBackendKind::Wgpu => crate::accel::gpu::wgpu::compute::cosine_distance(left, right),
        }
    }

    fn batch_squared_l2(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors.iter().map(|v| self.squared_l2_distance(query, v)).collect()
    }

    fn batch_inner_product(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors.iter().map(|v| self.inner_product_distance(query, v)).collect()
    }

    fn batch_cosine_distance(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors.iter().map(|v| self.cosine_distance(query, v)).collect()
    }

    fn crc32c_batch(&self, buffers: &[&[u8]]) -> Vec<u32> {
        match self.device.backend {
            GpuBackendKind::Cuda => crate::accel::gpu::cuda::crc::crc32c_batch(buffers),
            _ => buffers.iter().map(|b| self.crc32c(b)).collect(),
        }
    }
}
