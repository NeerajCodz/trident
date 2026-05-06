use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::Duration;
use tempfile::tempdir;
use trident::bench::{LatencyDistribution, WorkloadGenerator, WorkloadPattern};
use trident::index::LsmIndex;
use trident::store::{IndexInsert, StorageEngine};

// ──────────────────────────────────────────────
// Benchmark: Sequential Write Performance
// ──────────────────────────────────────────────

fn bench_sequential_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_writes");
    group.measurement_time(Duration::from_secs(5));

    for size_kb in [1, 10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size_kb)),
            size_kb,
            |b, &size_kb| {
                b.iter_batched(
                    || {
                        let dir = tempdir().unwrap();
                        let index_dir = dir.path().join("indexes");
                        let mut engine = StorageEngine::open(dir.path(), 128 * 1024 * 1024).unwrap();
                        engine
                            .register_index(
                                "kv",
                                "default",
                                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
                            )
                            .unwrap();
                        (engine, dir)
                    },
                    |(mut engine, _dir)| {
                        let data = vec![0xaa_u8; size_kb * 1024];
                        for i in 0..100 {
                            engine
                                .put(
                                    &data,
                                    &[IndexInsert::new("kv", format!("key{i:06}").into_bytes())],
                                )
                                .unwrap();
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ──────────────────────────────────────────────
// Benchmark: Read Performance with Different Patterns
// ──────────────────────────────────────────────

fn bench_read_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_patterns");
    group.measurement_time(Duration::from_secs(5));

    let patterns = vec![
        ("sequential", WorkloadPattern::Sequential),
        ("uniform", WorkloadPattern::UniformRandom),
        (
            "hot_80_20",
            WorkloadPattern::HotKey {
                hotset_fraction: 0.8,
            },
        ),
    ];

    for (pattern_name, pattern) in patterns {
        group.bench_with_input(
            BenchmarkId::from_parameter(pattern_name),
            &pattern,
            |b, &pattern| {
                b.iter_batched(
                    || {
                        let dir = tempdir().unwrap();
                        let index_dir = dir.path().join("indexes");
                        let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();
                        engine
                            .register_index(
                                "kv",
                                "default",
                                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
                            )
                            .unwrap();

                        // Pre-populate with 1000 records
                        for i in 0..1000 {
                            engine
                                .put(
                                    format!("value_{i}").as_bytes(),
                                    &[IndexInsert::new("kv", format!("key{i:06}").into_bytes())],
                                )
                                .unwrap();
                        }

                        (engine, dir)
                    },
                    |(mut engine, _dir)| {
                        let mut workload_gen = WorkloadGenerator::new(pattern, 1000);
                        for _ in 0..1000 {
                            let key_idx = workload_gen.next_key();
                            let key = format!("key{key_idx:06}");
                            engine
                                .fetch_by_index("kv", key.as_bytes())
                                .ok();
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ──────────────────────────────────────────────
// Benchmark: Index Lookup Performance
// ──────────────────────────────────────────────

fn bench_index_lookups(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_lookups");
    group.measurement_time(Duration::from_secs(5));

    for key_count in [100, 1000, 10_000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(key_count),
            key_count,
            |b, &key_count| {
                b.iter_batched(
                    || {
                        let dir = tempdir().unwrap();
                        let index_dir = dir.path().join("indexes");
                        let mut engine = StorageEngine::open(dir.path(), 128 * 1024 * 1024).unwrap();
                        engine
                            .register_index(
                                "kv",
                                "default",
                                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
                            )
                            .unwrap();

                        // Pre-populate
                        for i in 0..key_count {
                            engine
                                .put(
                                    format!("data_{i}").as_bytes(),
                                    &[IndexInsert::new("kv", format!("k{i:08}").into_bytes())],
                                )
                                .unwrap();
                        }

                        (engine, dir)
                    },
                    |(engine, _dir)| {
                        for i in 0..(key_count / 10) {
                            let key = format!("k{i:08}");
                            let _ = black_box(engine.lookup_rid("kv", key.as_bytes()));
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ──────────────────────────────────────────────
// Benchmark: Large Value Handling
// ──────────────────────────────────────────────

fn bench_large_values(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_values");
    group.measurement_time(Duration::from_secs(10));

    for size_mb in [1, 10, 50].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}MB", size_mb)),
            size_mb,
            |b, &size_mb| {
                b.iter_batched(
                    || {
                        let dir = tempdir().unwrap();
                        let index_dir = dir.path().join("indexes");
                        let mut engine =
                            StorageEngine::open(dir.path(), 512 * 1024 * 1024).unwrap();
                        engine
                            .register_index(
                                "kv",
                                "default",
                                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
                            )
                            .unwrap();
                        (engine, dir)
                    },
                    |(mut engine, _dir)| {
                        let large_value = vec![0xaa_u8; size_mb * 1024 * 1024];
                        for i in 0..3 {
                            engine
                                .put(
                                    &large_value,
                                    &[IndexInsert::new("kv", format!("large_{i}").into_bytes())],
                                )
                                .unwrap();
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ──────────────────────────────────────────────
// Benchmark: Concurrent Write Patterns
// ──────────────────────────────────────────────

fn bench_concurrent_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_writes");
    group.measurement_time(Duration::from_secs(10));

    for thread_count in [2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_threads", thread_count)),
            thread_count,
            |b, &thread_count| {
                b.iter_batched(
                    || {
                        let dir = tempdir().unwrap();
                        let index_dir = dir.path().join("indexes");
                        let mut engine = StorageEngine::open(dir.path(), 128 * 1024 * 1024).unwrap();
                        engine
                            .register_index(
                                "kv",
                                "default",
                                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
                            )
                            .unwrap();
                        (engine, dir)
                    },
                    |(engine, _dir)| {
                        use std::sync::Arc;
                        use std::thread;

                        let engine = Arc::new(parking_lot::Mutex::new(engine));
                        let mut handles = Vec::new();

                        for tid in 0..thread_count {
                            let engine = Arc::clone(&engine);
                            let handle = thread::spawn(move || {
                                for i in 0..10 {
                                    let mut e = engine.lock();
                                    e.put(
                                        format!("data_t{}_i{}", tid, i).as_bytes(),
                                        &[IndexInsert::new(
                                            "kv",
                                            format!("key_t{}_i{}", tid, i).into_bytes(),
                                        )],
                                    )
                                    .unwrap();
                                }
                            });
                            handles.push(handle);
                        }

                        for handle in handles {
                            handle.join().unwrap();
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ──────────────────────────────────────────────
// Benchmark: Latency Distribution
// ──────────────────────────────────────────────

fn bench_latency_distribution(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency_distribution");
    group.sample_size(100); // Smaller sample size for latency benchmark

    group.bench_function("sequential_1k_writes", |b| {
        b.iter_batched(
            || {
                let dir = tempdir().unwrap();
                let index_dir = dir.path().join("indexes");
                let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();
                engine
                    .register_index(
                        "kv",
                        "default",
                        Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
                    )
                    .unwrap();
                (engine, dir)
            },
            |(mut engine, _dir)| {
                let mut dist = LatencyDistribution::new();
                for i in 0..1000 {
                    let start = std::time::Instant::now();
                    engine
                        .put(
                            format!("data_{i}").as_bytes(),
                            &[IndexInsert::new("kv", format!("k{i:06}").into_bytes())],
                        )
                        .unwrap();
                    let latency_us = start.elapsed().as_micros() as u64;
                    dist.record(latency_us);
                }
                eprintln!("{}", dist.format_report());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_sequential_writes,
    bench_read_patterns,
    bench_index_lookups,
    bench_large_values,
    bench_concurrent_writes,
    bench_latency_distribution
);

criterion_main!(benches);
