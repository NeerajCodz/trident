use tempfile::tempdir;
use trident::TridentEngine;
use trident::accel::gpu::{GpuAccelerator, GpuBackendKind};
use trident::accel::{Accelerator, CpuAccelerator};
use trident::config::{AcceleratorBackend, Compression, TridentConfig};

#[test]
fn gpu_backends_match_cpu_results_without_hardware() {
    let cpu = CpuAccelerator;
    let input = b"gpu-fallback-must-preserve-correctness";

    for backend in [
        GpuBackendKind::Cuda,
        GpuBackendKind::Vulkan,
        GpuBackendKind::Metal,
        GpuBackendKind::Wgpu,
    ] {
        let gpu = GpuAccelerator::new(backend);
        assert_eq!(gpu.crc32c(input), cpu.crc32c(input));
        assert_eq!(
            gpu.compare_keys(b"k1", b"k2"),
            cpu.compare_keys(b"k1", b"k2")
        );
        assert_eq!(
            gpu.columnar_eq_offsets(&[b"a".as_slice(), b"b".as_slice(), b"a".as_slice()], b"a"),
            cpu.columnar_eq_offsets(&[b"a".as_slice(), b"b".as_slice(), b"a".as_slice()], b"a")
        );

        let encoded = gpu.encode_block(Compression::Zstd, input).unwrap();
        let decoded = gpu.decode_block(Compression::Zstd, &encoded).unwrap();
        assert_eq!(decoded, input);
    }
}

#[test]
fn engine_opens_with_gpu_accelerator_backends() {
    for backend in [
        AcceleratorBackend::Cuda,
        AcceleratorBackend::Vulkan,
        AcceleratorBackend::Metal,
    ] {
        let dir = tempdir().unwrap();
        let mut config = TridentConfig::new(dir.path());
        config.accelerator = backend;

        TridentEngine::open(config).unwrap();
    }
}

#[test]
fn vector_kernel_validates_dimensions() {
    let left = [1.0_f32, 2.0, 3.0];
    let right = [2.0_f32, 4.0, 6.0];

    assert_eq!(
        trident::accel::gpu::kernels::squared_l2_distance(&left, &right),
        Some(14.0)
    );
    assert_eq!(
        trident::accel::gpu::kernels::squared_l2_distance(&left, &right[..2]),
        None
    );
}
