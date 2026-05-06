//! Advanced scenario tests for production operational patterns.
//!
//! Tests realistic workload scenarios including:
//! - Multi-index write patterns with consistency validation
//! - Recovery scenarios and durability guarantees
//! - Concurrent reader/writer patterns
//! - Index compaction under load

use std::sync::Arc;
use std::thread;
use tempfile::tempdir;
use trident::index::{BTreeIndex, LsmIndex};
use trident::store::{IndexInsert, StorageEngine};

// ──────────────────────────────────────────────
// Multi-Index Scenarios
// ──────────────────────────────────────────────

/// Scenario: Inserting a node with multiple index views
/// (e.g., SQL row + KV lookup + graph edge)
#[test]
fn scenario_multi_index_node_insertion() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();

    // Register multiple index types for different access patterns
    engine
        .register_index("kv", "default", Box::new(LsmIndex::open("kv", &index_dir).unwrap()))
        .unwrap();
    engine
        .register_index("btree", "default", Box::new(BTreeIndex::open("btree", &index_dir).unwrap()))
        .unwrap();

    // Simulate a node that is indexed in two ways
    let node_data = b"user:alice:age:30:email:alice@example.com";
    let rid = engine
        .put(
            node_data,
            &[
                IndexInsert::new("kv", b"user_alice".to_vec()),
                IndexInsert::new("btree", b"idx_email_alice@example.com".to_vec()),
            ],
        )
        .unwrap();

    // Verify both access paths work
    assert_eq!(engine.fetch(rid).unwrap(), node_data);
    assert_eq!(engine.lookup_rid("kv", b"user_alice").unwrap(), Some(rid));
    assert_eq!(
        engine
            .lookup_rid("btree", b"idx_email_alice@example.com")
            .unwrap(),
        Some(rid)
    );

    // KV lookup by first index
    let kv_result = engine.fetch_by_index("kv", b"user_alice").unwrap();
    assert_eq!(kv_result, Some(node_data.to_vec()));
}

// ──────────────────────────────────────────────
// Consistency Under Concurrent Writers
// ──────────────────────────────────────────────

/// Scenario: Multiple threads writing to different indexes concurrently
#[test]
fn scenario_concurrent_multi_index_writes() {
    let dir = Arc::new(tempdir().unwrap());
    let index_dir = dir.path().join("indexes");

    let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();
    engine
        .register_index("kv", "default", Box::new(LsmIndex::open("kv", &index_dir).unwrap()))
        .unwrap();
    engine
        .register_index("bt", "default", Box::new(BTreeIndex::open("bt", &index_dir).unwrap()))
        .unwrap();

    let engine = Arc::new(parking_lot::Mutex::new(engine));

    let mut handles = Vec::new();

    // Thread 1: KV writes
    {
        let engine = Arc::clone(&engine);
        let handle = thread::spawn(move || {
            for i in 0..100 {
                let mut e = engine.lock();
                e.put(
                    format!("kv_data_{i}").as_bytes(),
                    &[IndexInsert::new("kv", format!("kv_key_{i:03}").into_bytes())],
                )
                .unwrap();
            }
        });
        handles.push(handle);
    }

    // Thread 2: BTree writes
    {
        let engine = Arc::clone(&engine);
        let handle = thread::spawn(move || {
            for i in 0..100 {
                let mut e = engine.lock();
                e.put(
                    format!("bt_data_{i}").as_bytes(),
                    &[IndexInsert::new("bt", format!("bt_key_{i:03}").into_bytes())],
                )
                .unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let engine = engine.lock();
    let stats = engine.stats();
    assert_eq!(
        stats.live_records, 200,
        "should have 200 records (100 + 100) from concurrent writers"
    );
}

// ──────────────────────────────────────────────
// Stress: Large Values with Multiple Indexes
// ──────────────────────────────────────────────

/// Scenario: Inserting large documents indexed multiple ways
#[test]
fn scenario_large_document_multi_index() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 256 * 1024 * 1024).unwrap();

    engine
        .register_index("kv", "default", Box::new(LsmIndex::open("kv", &index_dir).unwrap()))
        .unwrap();
    engine
        .register_index("bt", "default", Box::new(BTreeIndex::open("bt", &index_dir).unwrap()))
        .unwrap();

    // Create a 10MB document
    let document = vec![0xaa_u8; 10 * 1024 * 1024];

    let rid = engine
        .put(
            &document,
            &[
                IndexInsert::new("kv", b"large_doc_key".to_vec()),
                IndexInsert::new("bt", b"idx_size_10mb".to_vec()),
            ],
        )
        .unwrap();

    // Verify retrieval
    let retrieved = engine.fetch(rid).unwrap();
    assert_eq!(retrieved.len(), 10 * 1024 * 1024);
    assert_eq!(retrieved, document);

    // Verify both index lookups work
    assert_eq!(engine.lookup_rid("kv", b"large_doc_key").unwrap(), Some(rid));
    assert_eq!(engine.lookup_rid("bt", b"idx_size_10mb").unwrap(), Some(rid));
}

// ──────────────────────────────────────────────
// Maintenance & Compaction Scenarios
// ──────────────────────────────────────────────

/// Scenario: Maintaining storage during active writes
#[test]
fn scenario_maintenance_during_writes() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();

    engine
        .register_index("kv", "default", Box::new(LsmIndex::open("kv", &index_dir).unwrap()))
        .unwrap();

    // Write initial data
    for i in 0..100 {
        engine
            .put(
                format!("data_{i}").as_bytes(),
                &[IndexInsert::new("kv", format!("key_{i:03}").into_bytes())],
            )
            .unwrap();
    }

    // Check maintenance suggestions
    let suggestions = engine.suggest_index_compactions(10);
    eprintln!("Maintenance suggestions: {:?}", suggestions);

    // Run compaction
    let _compaction_reports = engine.compact_indexes().unwrap();

    // Verify data integrity after compaction
    for i in [0, 50, 99] {
        let retrieved = engine
            .fetch_by_index("kv", format!("key_{i:03}").as_bytes())
            .unwrap();
        assert!(retrieved.is_some(), "record {i} should still be readable");
    }
}

// ──────────────────────────────────────────────
// Multi-Index Deletion Scenarios
// ──────────────────────────────────────────────

/// Scenario: Deleting a record from multiple indexes
#[test]
fn scenario_multi_index_deletion() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();

    engine
        .register_index("kv", "default", Box::new(LsmIndex::open("kv", &index_dir).unwrap()))
        .unwrap();
    engine
        .register_index("bt", "default", Box::new(BTreeIndex::open("bt", &index_dir).unwrap()))
        .unwrap();

    // Insert a record in both indexes
    let rid = engine
        .put(
            b"data_to_delete",
            &[
                IndexInsert::new("kv", b"delete_me_kv".to_vec()),
                IndexInsert::new("bt", b"delete_me_bt".to_vec()),
            ],
        )
        .unwrap();

    // Verify it exists in both
    assert_eq!(engine.lookup_rid("kv", b"delete_me_kv").unwrap(), Some(rid));
    assert_eq!(engine.lookup_rid("bt", b"delete_me_bt").unwrap(), Some(rid));

    // Delete from both indexes
    engine.delete_index("kv", b"delete_me_kv").unwrap();
    engine.delete_index("bt", b"delete_me_bt").unwrap();

    // Verify deleted from both (returns None)
    assert_eq!(engine.lookup_rid("kv", b"delete_me_kv").unwrap(), None);
    assert_eq!(engine.lookup_rid("bt", b"delete_me_bt").unwrap(), None);

    // But the record itself should still be readable (soft delete)
    assert_eq!(engine.fetch(rid).unwrap(), b"data_to_delete");
}

// ──────────────────────────────────────────────
// Recovery & Restart Scenarios
// ──────────────────────────────────────────────

/// Scenario: Engine restarts and recovers state
#[test]
fn scenario_engine_restart_recovery() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");

    // Phase 1: Write data with first engine instance
    {
        let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();
        engine
            .register_index("kv", "default", Box::new(LsmIndex::open("kv", &index_dir).unwrap()))
            .unwrap();

        for i in 0..50 {
            engine
                .put(
                    format!("data_{i}").as_bytes(),
                    &[IndexInsert::new("kv", format!("key_{i:03}").into_bytes())],
                )
                .unwrap();
        }
        // engine goes out of scope, closes
    }

    // Phase 2: Restart engine and verify data is still there
    {
        let mut engine = StorageEngine::open(dir.path(), 64 * 1024 * 1024).unwrap();
        engine
            .register_index("kv", "default", Box::new(LsmIndex::open("kv", &index_dir).unwrap()))
            .unwrap();

        let stats = engine.stats();
        assert_eq!(
            stats.live_records, 50,
            "after restart, should recover 50 records"
        );

        // Spot check some records
        for i in [0, 25, 49] {
            let result = engine
                .fetch_by_index("kv", format!("key_{i:03}").as_bytes())
                .unwrap();
            assert!(result.is_some(), "record {i} should be recovered after restart");
        }
    }
}
