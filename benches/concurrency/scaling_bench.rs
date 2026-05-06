use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use trident::bench::{LatencyDistribution, WorkloadGenerator, WorkloadPattern};
use trident::storage::lsm::LsmIndex;
use trident::store::{IndexInsert, StorageEngine};

fn open_engine(cache_bytes: usize) -> (StorageEngine, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), cache_bytes).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();
    (engine, dir)
}

fn bench_concurrency_scaling_to_64(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3c_concurrency_scaling");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(10);

    for thread_count in [1_usize, 2, 4, 8, 16, 32, 64] {
        group.bench_function(
            BenchmarkId::from_parameter(format!("{thread_count}_threads")),
            |b| {
                b.iter_batched(
                    || Arc::new(Mutex::new(open_engine(256 * 1024 * 1024).0)),
                    |engine| {
                        let mut handles = Vec::with_capacity(thread_count);
                        for thread_id in 0..thread_count {
                            let engine = Arc::clone(&engine);
                            handles.push(thread::spawn(move || {
                                for op_id in 0..64 {
                                    let data = vec![0xaa_u8; 1024];
                                    let key = format!("phase3c-t{thread_id:02}-op{op_id:06}")
                                        .into_bytes();
                                    let mut engine = engine.lock();
                                    engine.put(&data, &[IndexInsert::new("kv", key)]).unwrap();
                                }
                            }));
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

fn bench_regression_validation_fixture(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3c_regression_validation");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(20);

    for delay_us in [0_u64, 250, 1_000] {
        group.bench_function(
            BenchmarkId::from_parameter(format!("simulated_delay_{delay_us}us")),
            |b| {
                b.iter_batched(
                    || open_engine(128 * 1024 * 1024),
                    |(mut engine, _dir)| {
                        let mut latency = LatencyDistribution::new();
                        for i in 0..256 {
                            let start = std::time::Instant::now();
                            if delay_us > 0 {
                                std::thread::sleep(Duration::from_micros(delay_us));
                            }
                            let data = vec![0xbb_u8; 1024];
                            engine
                                .put(
                                    &data,
                                    &[IndexInsert::new(
                                        "kv",
                                        format!("regression-key-{i:06}").into_bytes(),
                                    )],
                                )
                                .unwrap();
                            latency.record(start.elapsed().as_micros() as u64);
                        }
                        eprintln!("{}", latency.format_report());
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_soak_profile_1m_ops(c: &mut Criterion) {
    if std::env::var_os("TRIDENT_INCLUDE_SOAK_BENCH").is_none() {
        return;
    }

    let mut group = c.benchmark_group("phase3c_soak_profile");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(10);

    group.bench_function("mixed_workload_1m_ops", |b| {
        b.iter_batched(
            || open_engine(512 * 1024 * 1024),
            |(mut engine, _dir)| {
                let mut workload = WorkloadGenerator::new(
                    WorkloadPattern::HotKey {
                        hotset_fraction: 0.8,
                    },
                    50_000,
                );
                for i in 0..1_000_000 {
                    let key_idx = workload.next_key();
                    let key = format!("soak-key-{key_idx:06}");
                    if i % 4 == 0 {
                        let _ = engine.fetch_by_index("kv", key.as_bytes());
                    } else {
                        let data = vec![(i % 251) as u8; 512];
                        engine
                            .put(&data, &[IndexInsert::new("kv", key.into_bytes())])
                            .unwrap();
                    }
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_concurrency_scaling_to_64,
    bench_regression_validation_fixture,
    bench_soak_profile_1m_ops
);

criterion_main!(benches);
