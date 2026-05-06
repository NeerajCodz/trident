use criterion::{Criterion, black_box, criterion_group, criterion_main};
use trident::accel::{Accelerator, CpuAccelerator};

fn bench_cpu_crc32c(c: &mut Criterion) {
    let accel = CpuAccelerator;
    let payload = vec![0xaa_u8; 1024 * 1024];

    c.bench_function("cpu_crc32c_1mb", |b| {
        b.iter(|| black_box(accel.crc32c(black_box(&payload))))
    });
}

criterion_group!(benches, bench_cpu_crc32c);
criterion_main!(benches);
