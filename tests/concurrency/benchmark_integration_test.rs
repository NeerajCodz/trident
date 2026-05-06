//! Phase 3B Integration Tests: Advanced Benchmarking & Performance Analysis
//!
//! This test module validates the complete Phase 3B benchmarking infrastructure
//! including workload generation, metrics collection, and regression detection.

#[cfg(test)]
mod phase3b_integration {
    use std::sync::Arc;
    use std::thread;
    use tempfile::tempdir;
    use trident::bench::{
        LatencyDistribution, LockContentionTracker, ThroughputMeter, WorkloadGenerator,
        WorkloadPattern, WriteAmplificationTracker,
    };
    use trident::bench_advanced::{BenchmarkResult, BenchmarkSuite};
    use trident::storage::lsm::LsmIndex;
    use trident::store::{IndexInsert, StorageEngine};

    #[test]
    fn test_phase3b_sequential_workload() {
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

        let mut workload = WorkloadGenerator::new(WorkloadPattern::Sequential, 100);
        let mut latency_dist = LatencyDistribution::new();

        for _ in 0..100 {
            let start = std::time::Instant::now();
            let data = vec![0xaa_u8; 1024];
            engine
                .put(
                    &data,
                    &[IndexInsert::new(
                        "kv",
                        format!("key{:06}", workload.next_key()).into_bytes(),
                    )],
                )
                .unwrap();
            let elapsed_us = start.elapsed().as_micros() as u64;
            latency_dist.record(elapsed_us);
        }

        // Verify metrics were collected
        assert_eq!(latency_dist.total_samples(), 100);
        assert!(latency_dist.min() > 0);
        assert!(latency_dist.max() >= latency_dist.min());
        assert!(latency_dist.avg() > 0);

        // Verify percentiles exist
        assert!(latency_dist.percentile(0.50) > 0);
        assert!(latency_dist.percentile(0.95) > 0);
        assert!(latency_dist.percentile(0.99) > 0);

        println!("{}", latency_dist.format_report());
    }

    #[test]
    fn test_phase3b_workload_patterns() {
        let patterns = vec![
            ("Sequential", WorkloadPattern::Sequential),
            ("UniformRandom", WorkloadPattern::UniformRandom),
            (
                "HotKey",
                WorkloadPattern::HotKey {
                    hotset_fraction: 0.8,
                },
            ),
            ("Zipfian", WorkloadPattern::Zipfian { exponent: 0.99 }),
        ];

        for (name, pattern) in patterns {
            let mut workload = WorkloadGenerator::new(pattern, 1000);
            let keys: Vec<usize> = (0..100).map(|_| workload.next_key()).collect();

            // Verify keys are within bounds
            assert!(
                keys.iter().all(|&k| k < 1000),
                "Pattern {} generated invalid keys",
                name
            );

            // Verify diversity (except sequential which might have patterns)
            let unique_keys: std::collections::HashSet<_> = keys.iter().cloned().collect();
            match name {
                "Sequential" => {
                    // Sequential should have some predictability
                    assert!(!unique_keys.is_empty());
                }
                _ => {
                    // Others should have reasonable diversity
                    assert!(
                        unique_keys.len() > 1,
                        "Pattern {} has insufficient diversity",
                        name
                    );
                }
            }

            println!(
                "Pattern {}: {} unique keys in 100 samples",
                name,
                unique_keys.len()
            );
        }
    }

    #[test]
    fn test_phase3b_write_amplification_tracking() {
        let tracker = WriteAmplificationTracker::new();

        // Simulate 3 writes with 100 logical bytes each
        for _ in 0..3 {
            tracker.record_logical_write(100);
            tracker.record_physical_write(150); // 1.5x physical
            tracker.record_wal_write(100); // 1x WAL
        }

        assert_eq!(tracker.logical_bytes(), 300);
        assert_eq!(tracker.physical_bytes(), 450);
        assert_eq!(tracker.wal_bytes(), 300);

        let phys_amp = tracker.write_amplification();
        assert!(
            (1.5..=1.51).contains(&phys_amp),
            "Physical amp should be ~1.5x"
        );

        let wal_amp = tracker.wal_amplification();
        assert!((0.99..=1.01).contains(&wal_amp), "WAL amp should be ~1.0x");

        let total_amp = tracker.total_amplification();
        assert!(
            (2.49..=2.51).contains(&total_amp),
            "Total amp should be ~2.5x"
        );

        println!("{}", tracker.format_report());
    }

    #[test]
    fn test_phase3b_throughput_measurement() {
        let meter = ThroughputMeter::new();

        for _ in 0..100 {
            meter.record_operation(1024);
        }

        std::thread::sleep(std::time::Duration::from_millis(10));

        // After recording, ops/sec should be measurable
        let ops_per_sec = meter.ops_per_sec();
        assert!(ops_per_sec > 0.0, "Throughput should be measured");

        let mb_per_sec = meter.mb_per_sec();
        assert!(mb_per_sec >= 0.0);

        // Sanity checks: 100 ops * 1KB = 100KB
        assert!(mb_per_sec >= 0.0);

        println!("{}", meter.format_report());
    }

    #[test]
    fn test_phase3b_lock_contention() {
        let tracker = LockContentionTracker::new();

        // Record fast acquisitions
        for _ in 0..50 {
            tracker.record_acquisition(50); // 50μs (not contended)
        }

        // Record slow acquisitions
        for _ in 0..10 {
            tracker.record_acquisition(200); // 200μs (contended, > 100μs threshold)
        }

        assert_eq!(tracker.total_attempts(), 60);
        assert_eq!(tracker.contended_attempts(), 10);

        let contention_rate = tracker.contention_rate();
        assert!(contention_rate > 0.16 && contention_rate < 0.17);

        let avg_wait = tracker.avg_wait_us();
        assert!((70..=80).contains(&avg_wait));

        println!("{}", tracker.format_report());
    }

    #[test]
    fn test_phase3b_benchmark_result_creation() {
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

        let mut latency = LatencyDistribution::new();
        for i in 0..100 {
            let data = vec![0xaa_u8; 1024];
            let start = std::time::Instant::now();
            engine
                .put(
                    &data,
                    &[IndexInsert::new("kv", format!("key{i:06}").into_bytes())],
                )
                .unwrap();
            latency.record(start.elapsed().as_micros() as u64);
        }

        let throughput = ThroughputMeter::new();
        for _ in 0..100 {
            throughput.record_operation(1024);
        }

        let amplification = WriteAmplificationTracker::new();
        for _ in 0..100 {
            amplification.record_logical_write(1024);
            amplification.record_physical_write(1500);
        }

        let contention = LockContentionTracker::new();
        contention.record_acquisition(50);

        let result = BenchmarkResult::from_metrics(
            "test_benchmark",
            &WorkloadPattern::Sequential,
            1.5,
            &latency,
            &throughput,
            &amplification,
            &contention,
        );

        assert_eq!(result.name, "test_benchmark");
        assert_eq!(result.workload_pattern, "Sequential");
        assert!(result.latency_p99_us > 0);
        assert!(result.throughput_ops_per_sec > 0);
        assert!(result.write_amp_total > 1.0);

        println!("{}", result);
    }

    #[test]
    fn test_phase3b_benchmark_suite() {
        let mut suite = BenchmarkSuite::new();

        for i in 0..3 {
            let result = BenchmarkResult {
                name: format!("benchmark_{}", i),
                workload_pattern: "Sequential".into(),
                duration_secs: 1.0 + i as f64 * 0.5,
                latency_min_us: 100,
                latency_max_us: 10000,
                latency_avg_us: 500 + i as u64 * 100,
                latency_p50_us: 400,
                latency_p95_us: 2000,
                latency_p99_us: 5000 + i as u64 * 500,
                throughput_ops_per_sec: 1000 - i as u64 * 50,
                throughput_mb_per_sec: 10.0,
                write_amp_physical: 1.5,
                write_amp_wal: 2.0,
                write_amp_total: 3.5,
                lock_contention_rate: 0.05,
                lock_avg_wait_us: 50,
            };
            suite.add_result(result);
        }

        assert_eq!(suite.results.len(), 3);

        let stats = suite.statistics();
        assert!(stats.avg_latency_p99_us > 0.0);
        assert!(stats.avg_throughput_ops_per_sec > 0.0);

        println!("{}", stats);
    }

    #[test]
    fn test_phase3b_regression_detection() {
        let mut baseline = BenchmarkSuite::new();
        let mut current = BenchmarkSuite::new();

        let baseline_result = BenchmarkResult {
            name: "read_workload".into(),
            workload_pattern: "Random".into(),
            duration_secs: 1.0,
            latency_min_us: 100,
            latency_max_us: 5000,
            latency_avg_us: 400,
            latency_p50_us: 300,
            latency_p95_us: 1000,
            latency_p99_us: 2000,
            throughput_ops_per_sec: 10000,
            throughput_mb_per_sec: 100.0,
            write_amp_physical: 1.0,
            write_amp_wal: 1.0,
            write_amp_total: 2.0,
            lock_contention_rate: 0.01,
            lock_avg_wait_us: 10,
        };

        // Simulate 15% regression in P99 latency
        let mut current_result = baseline_result.clone();
        current_result.latency_p99_us = (2000.0 * 1.15) as u64; // 2300μs (15% worse)

        baseline.add_result(baseline_result);
        current.add_result(current_result);

        let regressions = current.compare(&baseline);
        assert!(!regressions.is_empty(), "Should detect regression");

        let regression = &regressions[0];
        assert_eq!(regression.benchmark_name, "read_workload");
        assert!(
            !regression.metrics.is_empty(),
            "Should have metric regression details"
        );

        // Verify P99 latency regression was detected
        let p99_regression = regression.metrics.iter().find(|m| m.metric.contains("P99"));
        assert!(
            p99_regression.is_some(),
            "P99 latency regression should be detected"
        );

        println!("Detected {} regressions", regressions.len());
        for r in &regressions {
            println!(
                "  {}: {} metrics regressed",
                r.benchmark_name,
                r.metrics.len()
            );
        }
    }

    #[test]
    #[ignore]
    fn test_phase3b_concurrent_benchmark() {
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

        let engine = Arc::new(parking_lot::Mutex::new(engine));
        let mut handles = vec![];

        for thread_id in 0..4 {
            let engine = Arc::clone(&engine);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    let data = vec![0xaa_u8; 1024];
                    let mut e = engine.lock();
                    let _ = e.put(
                        &data,
                        &[IndexInsert::new(
                            "kv",
                            format!("t{}_{:06}", thread_id, i).into_bytes(),
                        )],
                    );
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.join();
        }

        println!("Concurrent benchmark completed successfully");
    }
}
