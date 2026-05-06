use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use trident::accel::cpu::SimdCpuAccelerator;
use trident::accel::{Accelerator, CpuAccelerator};

fn generate_f32_vectors(dim: usize, count: usize) -> Vec<Vec<f32>> {
    let mut state = 42u64;
    (0..count)
        .map(|_| {
            (0..dim)
                .map(|_| {
                    state = state
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    ((state >> 33) as f32) / (u32::MAX as f32) * 2.0 - 1.0
                })
                .collect()
        })
        .collect()
}

fn bench_squared_l2(c: &mut Criterion) {
    let mut group = c.benchmark_group("squared_l2_distance");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    for dim in [32, 128, 512, 1024] {
        let vecs = generate_f32_vectors(dim, 2);
        let left = &vecs[0];
        let right = &vecs[1];

        group.bench_with_input(BenchmarkId::new("scalar", dim), &dim, |b, _| {
            b.iter(|| black_box(scalar.squared_l2_distance(black_box(left), black_box(right))))
        });

        group.bench_with_input(BenchmarkId::new("simd", dim), &dim, |b, _| {
            b.iter(|| black_box(simd.squared_l2_distance(black_box(left), black_box(right))))
        });
    }
    group.finish();
}

fn bench_inner_product(c: &mut Criterion) {
    let mut group = c.benchmark_group("inner_product_distance");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    for dim in [32, 128, 512, 1024] {
        let vecs = generate_f32_vectors(dim, 2);
        let left = &vecs[0];
        let right = &vecs[1];

        group.bench_with_input(BenchmarkId::new("scalar", dim), &dim, |b, _| {
            b.iter(|| black_box(scalar.inner_product_distance(black_box(left), black_box(right))))
        });

        group.bench_with_input(BenchmarkId::new("simd", dim), &dim, |b, _| {
            b.iter(|| black_box(simd.inner_product_distance(black_box(left), black_box(right))))
        });
    }
    group.finish();
}

fn bench_cosine_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("cosine_distance");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    for dim in [32, 128, 512, 1024] {
        let vecs = generate_f32_vectors(dim, 2);
        let left = &vecs[0];
        let right = &vecs[1];

        group.bench_with_input(BenchmarkId::new("scalar", dim), &dim, |b, _| {
            b.iter(|| black_box(scalar.cosine_distance(black_box(left), black_box(right))))
        });

        group.bench_with_input(BenchmarkId::new("simd", dim), &dim, |b, _| {
            b.iter(|| black_box(simd.cosine_distance(black_box(left), black_box(right))))
        });
    }
    group.finish();
}

fn bench_batch_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_squared_l2");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    let dim = 128;
    for batch_size in [10, 100, 1000] {
        let query_vec = generate_f32_vectors(dim, 1);
        let query = &query_vec[0];
        let dataset = generate_f32_vectors(dim, batch_size);
        let refs: Vec<&[f32]> = dataset.iter().map(Vec::as_slice).collect();

        group.bench_with_input(
            BenchmarkId::new("scalar", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| black_box(scalar.batch_squared_l2(black_box(query), black_box(&refs))))
            },
        );

        group.bench_with_input(BenchmarkId::new("simd", batch_size), &batch_size, |b, _| {
            b.iter(|| black_box(simd.batch_squared_l2(black_box(query), black_box(&refs))))
        });
    }
    group.finish();
}

fn bench_key_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_compare");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    for key_len in [8, 32, 128, 512] {
        let left: Vec<u8> = (0..key_len).map(|i| (i % 256) as u8).collect();
        let right: Vec<u8> = (0..key_len).map(|i| (i % 256) as u8).collect();

        group.bench_with_input(BenchmarkId::new("scalar", key_len), &key_len, |b, _| {
            b.iter(|| black_box(scalar.compare_keys(black_box(&left), black_box(&right))))
        });

        group.bench_with_input(BenchmarkId::new("simd", key_len), &key_len, |b, _| {
            b.iter(|| black_box(simd.compare_keys(black_box(&left), black_box(&right))))
        });
    }
    group.finish();
}

fn bench_bloom_probe(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_probe_batch");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    let filter = vec![0xAA_u8; 4096];
    for num_keys in [10, 100, 1000] {
        let keys: Vec<Vec<u8>> = (0..num_keys)
            .map(|i| format!("key-{i:06}").into_bytes())
            .collect();
        let refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();

        group.bench_with_input(BenchmarkId::new("scalar", num_keys), &num_keys, |b, _| {
            b.iter(|| black_box(scalar.bloom_probe_batch(black_box(&filter), black_box(&refs))))
        });

        group.bench_with_input(BenchmarkId::new("simd", num_keys), &num_keys, |b, _| {
            b.iter(|| black_box(simd.bloom_probe_batch(black_box(&filter), black_box(&refs))))
        });
    }
    group.finish();
}

fn bench_columnar_u64_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("columnar_u64_range");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    for n in [1000, 10_000, 100_000] {
        let values: Vec<u64> = (0..n).map(|i| i as u64).collect();

        group.bench_with_input(BenchmarkId::new("scalar", n), &n, |b, _| {
            b.iter(|| black_box(scalar.columnar_u64_range_mask(black_box(&values), 100, 500)))
        });

        group.bench_with_input(BenchmarkId::new("simd", n), &n, |b, _| {
            b.iter(|| black_box(simd.columnar_u64_range_mask(black_box(&values), 100, 500)))
        });
    }
    group.finish();
}

fn bench_columnar_f32_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("columnar_f32_range");
    let simd = SimdCpuAccelerator::default();

    for n in [1000, 10_000, 100_000] {
        let values: Vec<f32> = (0..n).map(|i| i as f32).collect();

        group.bench_with_input(BenchmarkId::new("simd", n), &n, |b, _| {
            b.iter(|| black_box(simd.columnar_f32_range_mask(black_box(&values), 100.0, 500.0)))
        });
    }
    group.finish();
}

fn bench_crc32c_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("crc32c_batch");
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    for num_bufs in [10, 100, 1000] {
        let buffers: Vec<Vec<u8>> = (0..num_bufs).map(|_| vec![0xBB_u8; 4096]).collect();
        let refs: Vec<&[u8]> = buffers.iter().map(Vec::as_slice).collect();

        group.bench_with_input(BenchmarkId::new("scalar", num_bufs), &num_bufs, |b, _| {
            b.iter(|| black_box(scalar.crc32c_batch(black_box(&refs))))
        });

        group.bench_with_input(BenchmarkId::new("simd", num_bufs), &num_bufs, |b, _| {
            b.iter(|| black_box(simd.crc32c_batch(black_box(&refs))))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_squared_l2,
    bench_inner_product,
    bench_cosine_distance,
    bench_batch_distance,
    bench_key_compare,
    bench_bloom_probe,
    bench_columnar_u64_range,
    bench_columnar_f32_range,
    bench_crc32c_batch,
);
criterion_main!(benches);
