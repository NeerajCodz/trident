use criterion::{Criterion, black_box, criterion_group, criterion_main};
use praxis::accel::{Accelerator, CpuAccelerator};

fn bench_columnar_filter(c: &mut Criterion) {
    let accel = CpuAccelerator;
    let values: Vec<Vec<u8>> = (0..100_000)
        .map(|i| {
            if i % 10 == 0 {
                b"target".to_vec()
            } else {
                format!("value-{i}").into_bytes()
            }
        })
        .collect();
    let refs: Vec<&[u8]> = values.iter().map(Vec::as_slice).collect();

    c.bench_function("accel_columnar_eq_offsets", |b| {
        b.iter(|| black_box(accel.columnar_eq_offsets(black_box(&refs), black_box(b"target"))))
    });
}

criterion_group!(benches, bench_columnar_filter);
criterion_main!(benches);
