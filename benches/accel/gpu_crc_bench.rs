use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use trident::accel::gpu::{GpuAccelerator, GpuBackendKind};
use trident::accel::Accelerator;

fn bench_gpu_crc32c_fallbacks(c: &mut Criterion) {
    let payload = vec![0xbb_u8; 1024 * 1024];
    let mut group = c.benchmark_group("gpu_crc32c_fallback");

    for backend in [
        GpuBackendKind::Cuda,
        GpuBackendKind::Vulkan,
        GpuBackendKind::Metal,
        GpuBackendKind::Wgpu,
    ] {
        let accel = GpuAccelerator::new(backend);
        group.bench_function(BenchmarkId::from_parameter(backend.label()), |b| {
            b.iter(|| black_box(accel.crc32c(black_box(&payload))))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_gpu_crc32c_fallbacks);
criterion_main!(benches);
