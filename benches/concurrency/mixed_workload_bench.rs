use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use parking_lot::Mutex;
use praxis::bench::{LatencyDistribution, WorkloadGenerator, WorkloadPattern};
use praxis::storage::lsm::LsmIndex;
use praxis::store::{IndexInsert, StorageEngine};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;
use tempfile::tempdir;

// ──────────────────────────────────────────────
// Phase 3B: Workload Patterns & Value Scaling
// ──────────────────────────────────────────────

fn bench_workload_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("workload_patterns");
    group.measurement_time(std::time::Duration::from_secs(3));

    let patterns = vec![
        ("sequential", WorkloadPattern::Sequential),
        ("uniform_random", WorkloadPattern::UniformRandom),
        (
            "hot_key",
            WorkloadPattern::HotKey {
                hotset_fraction: 0.8,
            },
        ),
        ("zipfian", WorkloadPattern::Zipfian { exponent: 0.99 }),
    ];

    for (name, pattern) in patterns {
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
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
                    let mut workload_gen = WorkloadGenerator::new(pattern, 1000);
                    for _ in 0..1000 {
                        let key_idx = workload_gen.next_key();
                        let data = vec![0xaa_u8; 1024];
                        let _ = engine.put(
                            &data,
                            &[IndexInsert::new(
                                "kv",
                                format!("key{key_idx:06}").into_bytes(),
                            )],
                        );
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ──────────────────────────────────────────────
// Phase 3B: Value Size Scaling
// ──────────────────────────────────────────────

fn bench_value_size_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("value_size_scaling");
    group.measurement_time(std::time::Duration::from_secs(2));

    let sizes = vec![
        ("1KB", 1024),
        ("10KB", 10240),
        ("100KB", 102400),
        ("1MB", 1024 * 1024),
    ];

    for (label, size) in sizes {
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter_batched(
                || {
                    let dir = tempdir().unwrap();
                    let index_dir = dir.path().join("indexes");
                    let mut engine = StorageEngine::open(dir.path(), 256 * 1024 * 1024).unwrap();
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
                    let data = vec![0xaa_u8; size];
                    for i in 0..10 {
                        let _ = engine.put(
                            &data,
                            &[IndexInsert::new("kv", format!("key{i:06}").into_bytes())],
                        );
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ──────────────────────────────────────────────
// Phase 3B: Latency Distribution Analysis
// ──────────────────────────────────────────────

fn bench_latency_distribution_comprehensive(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency_analysis");
    group.sample_size(100);

    group.bench_function("sequential_1000_ops", |b| {
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
                let mut dist = LatencyDistribution::new();
                for i in 0..1000 {
                    let start = Instant::now();
                    let data = vec![0xaa_u8; 1024];
                    let _ = engine.put(
                        &data,
                        &[IndexInsert::new("kv", format!("key{i:06}").into_bytes())],
                    );
                    let latency_us = start.elapsed().as_micros() as u64;
                    dist.record(latency_us);
                }
                // Print latency report for analysis
                eprintln!("{}", dist.format_report());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ──────────────────────────────────────────────
// Phase 3B: Write Amplification Measurement
// ──────────────────────────────────────────────

fn bench_write_amplification(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_amplification");
    group.measurement_time(std::time::Duration::from_secs(2));

    group.bench_function("sequential_writes", |b| {
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
                for i in 0..100 {
                    let data = vec![0xaa_u8; 10240];
                    let _ = engine.put(
                        &data,
                        &[IndexInsert::new("kv", format!("key{i:06}").into_bytes())],
                    );
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ──────────────────────────────────────────────
// Phase 3B: Concurrent Access Patterns
// ──────────────────────────────────────────────

fn bench_concurrent_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_access");
    group.measurement_time(std::time::Duration::from_secs(3));

    let thread_counts = vec![1, 2, 4, 8];

    for num_threads in thread_counts {
        group.bench_function(
            BenchmarkId::from_parameter(format!("{}_threads", num_threads)),
            |b| {
                b.iter_batched(
                    || {
                        let dir = tempdir().unwrap();
                        let index_dir = dir.path().join("indexes");
                        let mut engine =
                            StorageEngine::open(dir.path(), 256 * 1024 * 1024).unwrap();
                        engine
                            .register_index(
                                "kv",
                                "default",
                                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
                            )
                            .unwrap();
                        Arc::new(Mutex::new(engine))
                    },
                    |engine| {
                        let stop_flag = Arc::new(AtomicBool::new(false));
                        let mut handles = vec![];

                        for _ in 0..num_threads {
                            let engine = Arc::clone(&engine);
                            let stop_flag = Arc::clone(&stop_flag);

                            let handle = thread::spawn(move || {
                                for i in 0..100 {
                                    if stop_flag.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    let data = vec![0xaa_u8; 1024];
                                    let mut e = engine.lock();
                                    let _ = e.put(
                                        &data,
                                        &[IndexInsert::new(
                                            "kv",
                                            format!("key{i:06}").into_bytes(),
                                        )],
                                    );
                                }
                            });
                            handles.push(handle);
                        }

                        for handle in handles {
                            let _ = handle.join();
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
// Phase 3B: Hot-Key Performance Analysis
// ──────────────────────────────────────────────

fn bench_hot_key_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_key_analysis");
    group.sample_size(50);

    let hotset_fractions = vec![0.5, 0.7, 0.9];

    for fraction in hotset_fractions {
        group.bench_function(
            BenchmarkId::from_parameter(format!("{:.0}% hot", fraction * 100.0)),
            |b| {
                b.iter_batched(
                    || {
                        let dir = tempdir().unwrap();
                        let index_dir = dir.path().join("indexes");
                        let mut engine =
                            StorageEngine::open(dir.path(), 128 * 1024 * 1024).unwrap();
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
                        let mut workload_gen = WorkloadGenerator::new(
                            WorkloadPattern::HotKey {
                                hotset_fraction: fraction,
                            },
                            1000,
                        );
                        for _ in 0..500 {
                            let key_idx = workload_gen.next_key();
                            let data = vec![0xaa_u8; 1024];
                            let _ = engine.put(
                                &data,
                                &[IndexInsert::new(
                                    "kv",
                                    format!("key{key_idx:06}").into_bytes(),
                                )],
                            );
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// Register all benchmarks
criterion_group!(
    benches,
    bench_workload_patterns,
    bench_value_size_scaling,
    bench_latency_distribution_comprehensive,
    bench_write_amplification,
    bench_concurrent_access,
    bench_hot_key_performance
);

criterion_main!(benches);
