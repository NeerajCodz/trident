use crate::config::AcceleratorBackend;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuBackendKind {
    Cuda,
    Vulkan,
    Metal,
    Wgpu,
}

impl GpuBackendKind {
    pub fn from_accelerator_backend(backend: AcceleratorBackend) -> Option<Self> {
        match backend {
            AcceleratorBackend::Cpu => None,
            AcceleratorBackend::Cuda => Some(Self::Cuda),
            AcceleratorBackend::Vulkan => Some(Self::Vulkan),
            AcceleratorBackend::Metal => Some(Self::Metal),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
            Self::Metal => "metal",
            Self::Wgpu => "wgpu",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuDevice {
    pub backend: GpuBackendKind,
    pub hardware_available: bool,
    pub description: &'static str,
}

impl GpuDevice {
    pub fn detect(backend: GpuBackendKind) -> Self {
        let hardware_available = match backend {
            GpuBackendKind::Cuda => crate::accel::gpu::cuda::hardware_available(),
            GpuBackendKind::Vulkan => crate::accel::gpu::vulkan::hardware_available(),
            GpuBackendKind::Metal => crate::accel::gpu::metal::hardware_available(),
            GpuBackendKind::Wgpu => crate::accel::gpu::wgpu::hardware_available(),
        };

        Self {
            backend,
            hardware_available,
            description: if hardware_available {
                "hardware backend enabled"
            } else {
                "cpu fallback backend"
            },
        }
    }
}
