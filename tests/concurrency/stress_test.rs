//! Stress and soak tests for Trident storage engine.
//!
//! These tests validate performance, correctness, and durability under
//! sustained load, variable workloads, and challenging scenarios.

use std::time::{Duration, Instant};
use tempfile::tempdir;
use trident::index::BTreeIndex;
use trident::storage::lsm::LsmIndex;
use trident::store::{BatchRecord, IndexInsert, RecordId, StorageEngine};

fn full_stress_enabled() -> bool {
    std::env::var("TRIDENT_FULL_STRESS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn stress_scale(full_count: usize, smoke_count: usize) -> usize {
    std::env::var("TRIDENT_STRESS_KEY_COUNT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count > 0)
        .unwrap_or_else(|| {
            if full_stress_enabled() {
                full_count
            } else {
                smoke_count
            }
        })
}

// ─────────────────────────────────────────
// 1. Throughput & Load Testing
// ─────────────────────────────────────────

/// Stress test with sequential writes.
///
/// Defaults to a CI-friendly smoke scale. Set `TRIDENT_FULL_STRESS=1` for the
/// full 100K workload, or `TRIDENT_STRESS_KEY_COUNT=<n>` for a custom scale.
#[test]
#[ignore] // Run with: cargo test -- --ignored stress_100k
fn stress_100k_sequential_writes() {
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

    let start = Instant::now();
    let mut last_rids = Vec::new();
    let key_count = stress_scale(100_000, 1_000);
    let sample_stride = (key_count / 10).max(1);

    let values: Vec<Vec<u8>> = (0..key_count)
        .map(|i| format!("record-{i:08}-{:0>64}", i).into_bytes())
        .collect();
    let indexes: Vec<Vec<IndexInsert>> = (0..key_count)
        .map(|i| vec![IndexInsert::new("kv", format!("k{i}").into_bytes())])
        .collect();
    let records: Vec<BatchRecord<'_>> = values
        .iter()
        .zip(indexes.iter())
        .map(|(value, inserts)| BatchRecord::new(value, inserts))
        .collect();
    let rids = engine.put_batch(&records).unwrap();
    for i in (0..key_count).step_by(sample_stride) {
        last_rids.push(rids[i]);
    }

    let elapsed = start.elapsed();
    let throughput = key_count as f64 / elapsed.as_secs_f64();

    eprintln!(
        "{} sequential writes in {:.2}s ({:.0} ops/sec)",
        key_count,
        elapsed.as_secs_f64(),
        throughput
    );

    // Verify a sample of written records.
    for (i, rid) in last_rids.iter().enumerate() {
        let sample_idx = i * sample_stride;
        let data = format!("record-{sample_idx:08}-{:0>64}", sample_idx);
        assert_eq!(
            engine.fetch(*rid).unwrap(),
            data.as_bytes(),
            "record at index {sample_idx} should be readable"
        );
    }
}

// ─────────────────────────────────────────
// 2. Value Size Scaling
// ─────────────────────────────────────────

/// Test handling of large values (>1MB).
#[test]
fn stress_large_values() {
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

    let large_value = vec![0xaa_u8; 2 * 1024 * 1024]; // 2MB
    let rid = engine
        .put(&large_value, &[IndexInsert::new("kv", b"large".to_vec())])
        .unwrap();

    let retrieved = engine.fetch(rid).unwrap();
    assert_eq!(retrieved.len(), 2 * 1024 * 1024);
    assert_eq!(retrieved, large_value);

    // Verify index lookup still works.
    assert_eq!(engine.lookup_rid("kv", b"large").unwrap(), Some(rid));
}

/// Test varying value sizes (100B to 1MB).
#[test]
fn stress_mixed_value_sizes() {
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

    let mut expected_records = 0;
    let sizes = [100, 1_000, 10_000, 100_000, 1_000_000];

    for (batch, size) in sizes.iter().enumerate() {
        for i in 0..10 {
            let value = vec![0xbb; *size];
            let key = format!("batch-{}-item-{}", batch, i).into_bytes();
            let rid = engine.put(&value, &[IndexInsert::new("kv", key)]).unwrap();

            let retrieved = engine.fetch(rid).unwrap();
            assert_eq!(retrieved.len(), *size);
            expected_records += 1;
        }
    }

    let stats = engine.stats();
    assert_eq!(
        stats.live_records, expected_records as u64,
        "should have {expected_records} records"
    );
}

// ─────────────────────────────────────────
// 3. Index Size Scaling
// ─────────────────────────────────────────

/// Test LSM with a large key count.
///
/// Defaults to a CI-friendly smoke scale. Set `TRIDENT_FULL_STRESS=1` for the
/// full 100K workload, or `TRIDENT_STRESS_KEY_COUNT=<n>` for a custom scale.
#[test]
#[ignore] // Run with: cargo test -- --ignored stress_lsm
fn stress_lsm_large_key_count() {
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

    let key_count = stress_scale(100_000, 1_000);
    let values: Vec<Vec<u8>> = (0..key_count)
        .map(|i| format!("v{i}").into_bytes())
        .collect();
    let indexes: Vec<Vec<IndexInsert>> = (0..key_count)
        .map(|i| vec![IndexInsert::new("kv", format!("k{i:08}").into_bytes())])
        .collect();
    let records: Vec<BatchRecord<'_>> = values
        .iter()
        .zip(indexes.iter())
        .map(|(value, inserts)| BatchRecord::new(value, inserts))
        .collect();
    let rids = engine.put_batch(&records).unwrap();

    let stats = engine.stats();
    assert_eq!(stats.live_records, key_count as u64);

    // Spot-check some lookups.
    for i in [0, key_count / 2, key_count - 1] {
        assert_eq!(
            engine
                .lookup_rid("kv", format!("k{i:08}").as_bytes())
                .unwrap(),
            Some(rids[i])
        );
    }
}

/// Test B-tree with a large key count.
///
/// Defaults to a CI-friendly smoke scale. Set `TRIDENT_FULL_STRESS=1` for the
/// full 50K workload, or `TRIDENT_STRESS_KEY_COUNT=<n>` for a custom scale.
#[test]
#[ignore] // Run with: cargo test -- --ignored stress_btree
fn stress_btree_large_key_count() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 128 * 1024 * 1024).unwrap();
    engine
        .register_index(
            "bt",
            "default",
            Box::new(BTreeIndex::open("bt", &index_dir).unwrap()),
        )
        .unwrap();

    let key_count = stress_scale(50_000, 1_000);
    let values: Vec<Vec<u8>> = (0..key_count)
        .map(|i| format!("row{i}").into_bytes())
        .collect();
    let indexes: Vec<Vec<IndexInsert>> = (0..key_count)
        .map(|i| vec![IndexInsert::new("bt", format!("id:{i:08}").into_bytes())])
        .collect();
    let records: Vec<BatchRecord<'_>> = values
        .iter()
        .zip(indexes.iter())
        .map(|(value, inserts)| BatchRecord::new(value, inserts))
        .collect();
    let _rids = engine.put_batch(&records).unwrap();

    let stats = engine.stats();
    assert_eq!(stats.live_records, key_count as u64);
}

// ─────────────────────────────────────────
// 4. Compaction Under Load
// ─────────────────────────────────────────

/// Test compaction while handling concurrent writes.
#[test]
fn stress_compaction_with_concurrent_writes() {
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

    // Write initial batch.
    for i in 0..1000 {
        engine
            .put(
                format!("data{i}").as_bytes(),
                &[IndexInsert::new("kv", format!("key{i}").into_bytes())],
            )
            .unwrap();
    }

    // Trigger compaction.
    let _reports = engine.compact_indexes().unwrap();

    // Verify reads still work after compaction.
    for i in [0, 500, 999] {
        assert_eq!(
            engine
                .lookup_rid("kv", format!("key{i}").as_bytes())
                .unwrap(),
            Some(RecordId(i as u64 + 1)) // RID is 1-indexed
        );
    }

    // Write more data after compaction.
    for i in 1000..2000 {
        engine
            .put(
                format!("data{i}").as_bytes(),
                &[IndexInsert::new("kv", format!("key{i}").into_bytes())],
            )
            .unwrap();
    }

    let stats = engine.stats();
    assert_eq!(stats.live_records, 2000);
}

// ─────────────────────────────────────────
// 5. Workload Pattern Simulation
// ─────────────────────────────────────────

/// Test hot-key access pattern (small set of frequently accessed keys).
#[test]
#[ignore] // Run with: cargo test -- --ignored stress_hot_key
fn stress_hot_key_workload() {
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

    // Create 100 hot keys.
    let mut hot_rids = Vec::new();
    for i in 0..100 {
        let rid = engine
            .put(
                format!("hot{i}").as_bytes(),
                &[IndexInsert::new("kv", format!("hot-key-{i}").into_bytes())],
            )
            .unwrap();
        hot_rids.push(rid);
    }

    // Simulate hot-key access (80/20 rule).
    let start = Instant::now();
    let mut accesses = 0;

    while start.elapsed() < Duration::from_millis(500) {
        for rid in &hot_rids {
            engine.fetch(*rid).ok();
            accesses += 1;
        }
    }

    eprintln!(
        "Hot-key workload: {} accesses in {:.2}s",
        accesses,
        start.elapsed().as_secs_f64()
    );
}

/// Test uniform random access pattern.
#[test]
#[ignore] // Run with: cargo test -- --ignored stress_uniform
fn stress_uniform_workload() {
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

    // Create 1000 records with uniform distribution.
    let mut rids = Vec::new();
    for i in 0..1000 {
        let rid = engine
            .put(
                format!("record{i}").as_bytes(),
                &[IndexInsert::new("kv", format!("key{i:04}").into_bytes())],
            )
            .unwrap();
        rids.push(rid);
    }

    // Random access across all keys.
    let start = Instant::now();
    let mut accesses = 0;
    let mut rng = 12345_u64; // Simple LCG PRNG

    while start.elapsed() < Duration::from_millis(500) {
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let idx = (rng as usize) % rids.len();
        engine.fetch(rids[idx]).ok();
        accesses += 1;
    }

    eprintln!(
        "Uniform workload: {} accesses in {:.2}s",
        accesses,
        start.elapsed().as_secs_f64()
    );
}
