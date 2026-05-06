//! Tests for Trident's no-duplication storage engine.
//!
//! The central invariant under test: **every value payload is written to the
//! primary [`RecordStore`] exactly once**.  All index plugins ([`LsmIndex`],
//! [`BTreeIndex`], [`AdjacencyIndex`], [`HnswIndex`]) store only
//! `key → RecordId` pointers and never duplicate the underlying bytes.

use tempfile::tempdir;
use trident::index::{AdjacencyIndex, BTreeIndex, HnswIndex, IndexPlugin, LsmIndex};
use trident::store::{
    IndexInsert, MaintenanceCycleOptions, RecordId, RecordStore, StorageEngine, StorageWal,
};

// ──────────────────────────────────────────────
// 1.  Basic RecordStore read / write / delete
// ──────────────────────────────────────────────

#[test]
fn record_store_write_and_read() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();

    let rid = store.put(b"hello world").unwrap();
    assert!(!rid.is_null());
    assert_eq!(store.get(rid).unwrap(), b"hello world");
}

#[test]
fn record_store_multiple_records_independent() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();

    let r1 = store.put(b"alpha").unwrap();
    let r2 = store.put(b"beta").unwrap();
    let r3 = store.put(b"gamma").unwrap();

    assert_ne!(r1, r2);
    assert_ne!(r2, r3);
    assert_eq!(store.get(r1).unwrap(), b"alpha");
    assert_eq!(store.get(r2).unwrap(), b"beta");
    assert_eq!(store.get(r3).unwrap(), b"gamma");
    assert_eq!(store.live_count(), 3);
}

#[test]
fn record_store_delete_marks_dead() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();

    let alive = store.put(b"keep").unwrap();
    let dead = store.put(b"discard").unwrap();

    store.delete(dead).unwrap();

    assert_eq!(store.get(alive).unwrap(), b"keep");
    assert!(
        store.get(dead).is_err(),
        "deleted record must not be readable"
    );
    assert_eq!(store.live_count(), 1);
}

// ──────────────────────────────────────────────
// 2.  Single-copy guarantee across multiple indexes
// ──────────────────────────────────────────────

/// Write a value once; point both an LSM and a B-tree index at the same RID.
/// The store must report exactly one live record.
#[test]
fn single_copy_per_value_lsm_and_btree() {
    let dir = tempdir().unwrap();
    let idir = dir.path().join("idx");

    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("kv", &idir).unwrap();
    let mut btree = BTreeIndex::open("bt", &idir).unwrap();

    let value = b"shared payload -- stored exactly once";
    let rid = store.put(value).unwrap();

    // Both indexes hold the same RID; the bytes are not duplicated.
    lsm.put(b"lsm-key", rid).unwrap();
    btree.put(b"btree-key", rid).unwrap();

    // One live record in the primary store regardless of index count.
    assert_eq!(store.live_count(), 1);
    assert_eq!(store.live_bytes(), value.len() as u64);

    // Both indexes resolve to the same RID.
    assert_eq!(lsm.get(b"lsm-key"), Some(rid));
    assert_eq!(btree.get(b"btree-key"), Some(rid));

    // Both lookups retrieve the same bytes through the store.
    assert_eq!(store.get(lsm.get(b"lsm-key").unwrap()).unwrap(), value);
    assert_eq!(store.get(btree.get(b"btree-key").unwrap()).unwrap(), value);
}

/// Write two values (Alice and Bob); index Alice by name (LSM), age (B-tree),
/// and a "friend" edge (adjacency).  The store must report exactly two live
/// records – one for Alice, one for Bob.
#[test]
fn three_index_types_two_data_records() {
    let dir = tempdir().unwrap();
    let idir = dir.path().join("idx");

    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("name", &idir).unwrap();
    let mut btree = BTreeIndex::open("age", &idir).unwrap();
    let mut adj = AdjacencyIndex::open("social", &idir).unwrap();

    let alice = store.put(b"alice-profile-data").unwrap();
    let bob = store.put(b"bob-profile-data").unwrap();

    // Three different index types all point to alice's single record.
    lsm.put(b"alice", alice).unwrap();
    btree.put(b"030:alice", alice).unwrap(); // age 30
    adj.add_edge(alice, b"friend", bob).unwrap();

    // Two live records total (alice + bob), not one per index entry.
    assert_eq!(store.live_count(), 2);

    // All indexes resolve correctly.
    assert_eq!(lsm.get(b"alice"), Some(alice));
    assert_eq!(btree.get(b"030:alice"), Some(alice));
    let neighbors = adj.neighbors(alice);
    assert_eq!(neighbors.len(), 1);
    assert_eq!(neighbors[0].to, bob);

    // Resolve through the store – same bytes, no extra copies.
    assert_eq!(
        store.get(lsm.get(b"alice").unwrap()).unwrap(),
        b"alice-profile-data"
    );
    assert_eq!(
        store.get(btree.get(b"030:alice").unwrap()).unwrap(),
        b"alice-profile-data"
    );
    assert_eq!(store.get(neighbors[0].to).unwrap(), b"bob-profile-data");
}

// ──────────────────────────────────────────────
// 3.  Compaction: RIDs remain stable
// ──────────────────────────────────────────────

/// After compaction the logical RID of a live record is unchanged.
/// All index plugin lookups must continue to return the correct data.
#[test]
fn compaction_preserves_rids_and_index_lookups() {
    let dir = tempdir().unwrap();
    let idir = dir.path().join("idx");

    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("kv", &idir).unwrap();

    let rid_a = store.put(b"alive").unwrap();
    let rid_b = store.put(b"dead").unwrap();

    lsm.put(b"a", rid_a).unwrap();
    lsm.put(b"b", rid_b).unwrap();

    // Delete "b" from the store; the LSM entry becomes stale (same as any
    // real secondary index on a deleted row).
    store.delete(rid_b).unwrap();

    let stats = store.compact().unwrap();
    assert_eq!(stats.records_retained, 1);
    assert_eq!(stats.records_dropped, 1);

    // RID for "a" is stable across compaction; the LSM index still works.
    let resolved = lsm.get(b"a").unwrap();
    assert_eq!(resolved, rid_a);
    assert_eq!(store.get(resolved).unwrap(), b"alive");

    // "b" is gone from the store.
    assert!(store.get(rid_b).is_err());
}

/// Compact an empty store (all records deleted).
#[test]
fn compaction_all_deleted_leaves_zero_records() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();

    let r1 = store.put(b"x").unwrap();
    let r2 = store.put(b"y").unwrap();
    store.delete(r1).unwrap();
    store.delete(r2).unwrap();

    let stats = store.compact().unwrap();
    assert_eq!(stats.records_retained, 0);
    assert_eq!(stats.records_dropped, 2);
    assert_eq!(store.live_count(), 0);
}

// ──────────────────────────────────────────────
// 4.  Persistence: store and indexes survive a restart
// ──────────────────────────────────────────────

#[test]
fn store_and_indexes_survive_reopen() {
    let dir = tempdir().unwrap();
    let idir = dir.path().join("idx");

    let rid = {
        let mut store = RecordStore::open(dir.path()).unwrap();
        let mut lsm = LsmIndex::open("kv", &idir).unwrap();
        let mut btree = BTreeIndex::open("bt", &idir).unwrap();

        let rid = store.put(b"persistent value").unwrap();
        lsm.put(b"key1", rid).unwrap();
        btree.put(b"key1", rid).unwrap();

        store.flush().unwrap();
        lsm.flush().unwrap();
        btree.flush().unwrap();
        rid
    };

    // Reopen everything from disk.
    let store = RecordStore::open(dir.path()).unwrap();
    let lsm = LsmIndex::open("kv", &idir).unwrap();
    let btree = BTreeIndex::open("bt", &idir).unwrap();

    let rid_lsm = lsm.get(b"key1").expect("LSM should survive reopen");
    let rid_bt = btree.get(b"key1").expect("B-tree should survive reopen");

    assert_eq!(rid_lsm, rid);
    assert_eq!(rid_bt, rid);
    assert_eq!(store.get(rid_lsm).unwrap(), b"persistent value");
}

// ──────────────────────────────────────────────
// 5.  Index-specific behaviour: LSM range scan
// ──────────────────────────────────────────────

#[test]
fn lsm_scan_returns_sorted_range() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("kv", dir.path().join("idx")).unwrap();

    let r1 = store.put(b"v1").unwrap();
    let r2 = store.put(b"v2").unwrap();
    let r3 = store.put(b"v3").unwrap();

    lsm.put(b"k1", r1).unwrap();
    lsm.put(b"k2", r2).unwrap();
    lsm.put(b"k3", r3).unwrap();

    // Half-open range [k1, k3) → k1 and k2 only.
    let results = lsm.scan(Some(b"k1"), Some(b"k3"));
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], (b"k1".to_vec(), r1));
    assert_eq!(results[1], (b"k2".to_vec(), r2));
}

#[test]
fn lsm_scan_open_bounds() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("kv", dir.path().join("idx")).unwrap();

    let r1 = store.put(b"a").unwrap();
    let r2 = store.put(b"b").unwrap();
    let r3 = store.put(b"c").unwrap();

    lsm.put(b"x", r1).unwrap();
    lsm.put(b"y", r2).unwrap();
    lsm.put(b"z", r3).unwrap();

    let all = lsm.scan(None, None);
    assert_eq!(all.len(), 3);
}

#[test]
fn lsm_delete_removes_entry_from_scan() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("kv", dir.path().join("idx")).unwrap();

    let r1 = store.put(b"keep").unwrap();
    let r2 = store.put(b"gone").unwrap();

    lsm.put(b"keep", r1).unwrap();
    lsm.put(b"gone", r2).unwrap();
    lsm.delete(b"gone").unwrap();

    let results = lsm.scan(None, None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, r1);
    assert_eq!(lsm.get(b"gone"), None);
}

#[test]
fn lsm_snapshot_get_at_respects_sequence() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("kv", dir.path().join("idx")).unwrap();

    let v1 = store.put(b"v1").unwrap();
    let v2 = store.put(b"v2").unwrap();

    lsm.put_with_sequence(b"k", v1, 10).unwrap();
    lsm.put_with_sequence(b"k", v2, 20).unwrap();

    assert_eq!(lsm.get_at(b"k", 9), None);
    assert_eq!(lsm.get_at(b"k", 10), Some(v1));
    assert_eq!(lsm.get_at(b"k", 19), Some(v1));
    assert_eq!(lsm.get_at(b"k", 20), Some(v2));
}

#[test]
fn lsm_bloom_and_fence_metadata_survive_reopen() {
    let dir = tempdir().unwrap();
    let idx_dir = dir.path().join("idx");

    {
        let mut store = RecordStore::open(dir.path()).unwrap();
        let mut lsm = LsmIndex::open("kv", &idx_dir).unwrap();
        let a = store.put(b"a").unwrap();
        let b = store.put(b"b").unwrap();
        lsm.put(b"k1", a).unwrap();
        lsm.put(b"k2", b).unwrap();
        lsm.flush().unwrap();
    }

    let lsm = LsmIndex::open("kv", &idx_dir).unwrap();
    assert!(lsm.may_contain_key(b"k1"));
    assert!(lsm.may_contain_key(b"k2"));
    assert!(!lsm.may_contain_key(b"k3"));
    let (min, max) = lsm.fence_bounds();
    assert_eq!(min.unwrap(), b"k1".to_vec());
    assert_eq!(max.unwrap(), b"k2".to_vec());
}

// ──────────────────────────────────────────────
// 6.  Index-specific behaviour: B-tree range scan
// ──────────────────────────────────────────────

#[test]
fn btree_range_scan_ordered() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut btree = BTreeIndex::open("bt", dir.path().join("idx")).unwrap();

    let r1 = store.put(b"v1").unwrap();
    let r2 = store.put(b"v2").unwrap();
    let r3 = store.put(b"v3").unwrap();

    btree.put(b"10", r1).unwrap();
    btree.put(b"20", r2).unwrap();
    btree.put(b"30", r3).unwrap();

    // [10, 30) → 10 and 20 only.
    let range = btree.range(Some(b"10"), Some(b"30"));
    assert_eq!(range.len(), 2);
    assert_eq!(range[0].1, r1);
    assert_eq!(range[1].1, r2);
}

#[test]
fn btree_open_lower_bound() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut btree = BTreeIndex::open("bt", dir.path().join("idx")).unwrap();

    let r1 = store.put(b"a").unwrap();
    let r2 = store.put(b"b").unwrap();
    let r3 = store.put(b"c").unwrap();

    btree.put(b"aa", r1).unwrap();
    btree.put(b"bb", r2).unwrap();
    btree.put(b"cc", r3).unwrap();

    // Unbounded lower, upper = b"cc" → aa and bb.
    let range = btree.range(None, Some(b"cc"));
    assert_eq!(range.len(), 2);
    assert_eq!(range[0].1, r1);
    assert_eq!(range[1].1, r2);
}

#[test]
fn btree_snapshot_get_at_respects_sequence() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut btree = BTreeIndex::open("bt", dir.path().join("idx")).unwrap();

    let old = store.put(b"old").unwrap();
    let new = store.put(b"new").unwrap();
    btree.put_with_sequence(b"id:1", old, 100).unwrap();
    btree.put_with_sequence(b"id:1", new, 200).unwrap();

    assert_eq!(btree.get_at(b"id:1", 150), Some(old));
    assert_eq!(btree.get_at(b"id:1", 200), Some(new));
}

#[test]
fn btree_page_metadata_survives_reopen() {
    let dir = tempdir().unwrap();
    let idx_dir = dir.path().join("idx");
    {
        let mut store = RecordStore::open(dir.path()).unwrap();
        let mut btree = BTreeIndex::open("bt", &idx_dir).unwrap();
        for i in 0..300 {
            let rid = store.put(format!("v{i}").as_bytes()).unwrap();
            btree.put(format!("k{i:03}").as_bytes(), rid).unwrap();
        }
        btree.flush().unwrap();
    }

    let btree = BTreeIndex::open("bt", &idx_dir).unwrap();
    let pages = btree.page_metadata();
    assert!(!pages.is_empty());
    assert!(
        pages.len() >= 3,
        "300 keys with default page size should span pages"
    );
    assert!(pages[0].min_key <= pages[0].max_key);
}

// ──────────────────────────────────────────────
// 7.  Index-specific behaviour: adjacency (graph)
// ──────────────────────────────────────────────

#[test]
fn adjacency_add_and_query_edges() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut adj = AdjacencyIndex::open("g", dir.path().join("idx")).unwrap();

    let alice = store.put(b"alice").unwrap();
    let bob = store.put(b"bob").unwrap();
    let carol = store.put(b"carol").unwrap();

    adj.add_edge(alice, b"follows", bob).unwrap();
    adj.add_edge(alice, b"follows", carol).unwrap();
    adj.add_edge(alice, b"blocks", bob).unwrap();

    let follows = adj.neighbors_with_label(alice, b"follows");
    assert_eq!(follows.len(), 2);
    assert!(follows.contains(&bob));
    assert!(follows.contains(&carol));

    let blocks = adj.neighbors_with_label(alice, b"blocks");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0], bob);

    assert_eq!(adj.neighbors(alice).len(), 3);
}

#[test]
fn adjacency_duplicate_edges_ignored() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut adj = AdjacencyIndex::open("g", dir.path().join("idx")).unwrap();

    let a = store.put(b"a").unwrap();
    let b = store.put(b"b").unwrap();

    adj.add_edge(a, b"rel", b).unwrap();
    adj.add_edge(a, b"rel", b).unwrap(); // duplicate – must be ignored
    adj.add_edge(a, b"rel", b).unwrap();

    assert_eq!(adj.neighbors(a).len(), 1);
}

#[test]
fn adjacency_remove_edges() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut adj = AdjacencyIndex::open("g", dir.path().join("idx")).unwrap();

    let a = store.put(b"a").unwrap();
    let b = store.put(b"b").unwrap();
    let c = store.put(b"c").unwrap();

    adj.add_edge(a, b"knows", b).unwrap();
    adj.add_edge(a, b"knows", c).unwrap();
    adj.remove_edges(a, b);

    let remaining = adj.neighbors(a);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].to, c);
}

#[test]
fn adjacency_add_bidirectional_edge() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut adj = AdjacencyIndex::open("g", dir.path().join("idx")).unwrap();

    let a = store.put(b"a").unwrap();
    let b = store.put(b"b").unwrap();
    adj.add_bidirectional_edge(a, b"follows", b, b"followed_by")
        .unwrap();

    assert_eq!(adj.neighbors_with_label(a, b"follows"), vec![b]);
    assert_eq!(adj.neighbors_with_label(b, b"followed_by"), vec![a]);
}

#[test]
fn adjacency_survives_reopen() {
    let dir = tempdir().unwrap();
    let idir = dir.path().join("idx");

    let (alice, bob) = {
        let mut store = RecordStore::open(dir.path()).unwrap();
        let mut adj = AdjacencyIndex::open("g", &idir).unwrap();
        let alice = store.put(b"alice").unwrap();
        let bob = store.put(b"bob").unwrap();
        adj.add_edge(alice, b"friend", bob).unwrap();
        adj.flush().unwrap();
        (alice, bob)
    };

    let adj = AdjacencyIndex::open("g", &idir).unwrap();
    let friends = adj.neighbors_with_label(alice, b"friend");
    assert_eq!(friends, vec![bob]);
}

// ──────────────────────────────────────────────
// 8.  Index-specific behaviour: HNSW vector index
// ──────────────────────────────────────────────

#[test]
fn hnsw_insert_and_search() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut hnsw = HnswIndex::new("vec");

    let doc_a = store.put(b"document A").unwrap();
    let doc_b = store.put(b"document B").unwrap();

    hnsw.insert(vec![1.0_f32, 0.0, 0.0], doc_a).unwrap();
    hnsw.insert(vec![0.0_f32, 1.0, 0.0], doc_b).unwrap();

    // Query close to doc_a.
    let results = hnsw.search(&[0.9, 0.1, 0.0], 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, doc_a);

    // Resolve through store – no body duplication in the index.
    assert_eq!(store.get(results[0].0).unwrap(), b"document A");
}

#[test]
fn hnsw_top_k_returns_k_results() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut hnsw = HnswIndex::new("vec");

    for i in 0..10_u8 {
        let rid = store.put(&[i]).unwrap();
        hnsw.insert(vec![i as f32, 0.0], rid).unwrap();
    }

    let results = hnsw.search(&[5.0, 0.0], 3);
    assert_eq!(results.len(), 3);
    // Results must be sorted by ascending distance.
    assert!(results[0].1 <= results[1].1);
    assert!(results[1].1 <= results[2].1);
}

#[test]
fn hnsw_mismatched_dimensions_skipped() {
    let mut hnsw = HnswIndex::new("vec");
    hnsw.insert(vec![1.0, 2.0], RecordId(1)).unwrap();
    hnsw.insert(vec![1.0, 2.0, 3.0], RecordId(2)).unwrap(); // dim=3

    // Query with dim=2 – only the dim=2 vector matches.
    let results = hnsw.search(&[0.0, 0.0], 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, RecordId(1));
}

#[test]
fn hnsw_persisted_index_survives_reopen() {
    let dir = tempdir().unwrap();
    let idx_dir = dir.path().join("idx");
    let mut store = RecordStore::open(dir.path()).unwrap();

    let rid = store.put(b"persisted-doc").unwrap();
    {
        let mut hnsw = HnswIndex::open("vec", &idx_dir).unwrap();
        hnsw.insert(vec![1.0, 2.0, 3.0], rid).unwrap();
        hnsw.flush().unwrap();
    }

    let hnsw = HnswIndex::open("vec", &idx_dir).unwrap();
    let results = hnsw.search(&[1.0, 2.0, 3.0], 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, rid);
}

// ──────────────────────────────────────────────
// 9.  Compaction with all index types still consistent
// ──────────────────────────────────────────────

#[test]
fn compaction_followed_by_all_index_queries() {
    let dir = tempdir().unwrap();
    let idir = dir.path().join("idx");

    let mut store = RecordStore::open(dir.path()).unwrap();
    let mut lsm = LsmIndex::open("lsm", &idir).unwrap();
    let mut btree = BTreeIndex::open("bt", &idir).unwrap();
    let mut adj = AdjacencyIndex::open("g", &idir).unwrap();

    let alice = store.put(b"alice").unwrap();
    let bob = store.put(b"bob").unwrap();
    let stale = store.put(b"will be deleted").unwrap();

    lsm.put(b"alice", alice).unwrap();
    btree.put(b"alice", alice).unwrap();
    adj.add_edge(alice, b"knows", bob).unwrap();

    store.delete(stale).unwrap();
    let stats = store.compact().unwrap();

    assert_eq!(stats.records_retained, 2);
    assert_eq!(stats.records_dropped, 1);

    // All indexes still work after compaction rewrote the segments.
    assert_eq!(store.get(lsm.get(b"alice").unwrap()).unwrap(), b"alice");
    assert_eq!(store.get(btree.get(b"alice").unwrap()).unwrap(), b"alice");
    let neighbors = adj.neighbors_with_label(alice, b"knows");
    assert_eq!(store.get(neighbors[0]).unwrap(), b"bob");
}

// ──────────────────────────────────────────────
// 10. RecordId properties
// ──────────────────────────────────────────────

#[test]
fn record_id_null_sentinel() {
    assert!(RecordId::NULL.is_null());
    assert_eq!(RecordId::NULL, RecordId(0));
}

#[test]
fn record_id_not_null_after_put() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();
    let rid = store.put(b"x").unwrap();
    assert!(!rid.is_null());
    assert_ne!(rid, RecordId::NULL);
}

#[test]
fn record_id_monotonically_increasing() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();

    let r1 = store.put(b"a").unwrap();
    let r2 = store.put(b"b").unwrap();
    let r3 = store.put(b"c").unwrap();

    assert!(r1 < r2);
    assert!(r2 < r3);
}

#[test]
fn storage_engine_writes_once_and_serves_multiple_indexes() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");

    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();
    engine
        .register_index(
            "pk",
            "default",
            Box::new(BTreeIndex::open("pk", &index_dir).unwrap()),
        )
        .unwrap();

    let rid = engine
        .put(
            b"single-copy-record",
            &[
                IndexInsert::new("kv", b"user:42".to_vec()),
                IndexInsert::new("pk", b"users/42".to_vec()),
            ],
        )
        .unwrap();

    assert_eq!(engine.live_count(), 1);
    assert_eq!(
        engine.lookup_rid("kv", b"user:42").unwrap(),
        Some(rid),
        "LSM index should map to the same logical RID"
    );
    assert_eq!(
        engine.lookup_rid("pk", b"users/42").unwrap(),
        Some(rid),
        "B-tree index should map to the same logical RID"
    );
    assert_eq!(engine.fetch(rid).unwrap(), b"single-copy-record");
}

#[test]
fn storage_engine_replays_wal_entries_by_index_type() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");

    let rid = {
        let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
        engine
            .register_index(
                "kv",
                "default",
                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
            )
            .unwrap();
        engine
            .register_index(
                "bt",
                "default",
                Box::new(BTreeIndex::open("bt", &index_dir).unwrap()),
            )
            .unwrap();
        let rid = engine
            .put(
                b"wal-backed-value",
                &[
                    IndexInsert::new("kv", b"k".to_vec()),
                    IndexInsert::new("bt", b"b".to_vec()),
                ],
            )
            .unwrap();

        // Simulate process crash before plugin snapshot flush; WAL must restore
        // index mappings on reopen.
        rid
    };

    let mut reopened = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    reopened
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();
    reopened
        .register_index(
            "bt",
            "default",
            Box::new(BTreeIndex::open("bt", &index_dir).unwrap()),
        )
        .unwrap();

    assert_eq!(reopened.lookup_rid("kv", b"k").unwrap(), Some(rid));
    assert_eq!(reopened.lookup_rid("bt", b"b").unwrap(), Some(rid));
    assert_eq!(reopened.fetch(rid).unwrap(), b"wal-backed-value");
}

#[test]
fn storage_engine_put_uses_grouped_wal_batch_sequences() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");

    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "lsm",
            "default",
            Box::new(LsmIndex::open("lsm", &index_dir).unwrap()),
        )
        .unwrap();
    engine
        .register_index(
            "bt",
            "default",
            Box::new(BTreeIndex::open("bt", &index_dir).unwrap()),
        )
        .unwrap();

    let rid = engine
        .put(
            b"group-commit-payload",
            &[
                IndexInsert::new("lsm", b"k".to_vec()),
                IndexInsert::new("bt", b"p".to_vec()),
            ],
        )
        .unwrap();
    drop(engine);

    let entries = StorageWal::replay(&dir.path().join("wal").join("storage.wal")).unwrap();
    assert_eq!(entries.len(), 3, "primary + two index entries");
    assert_eq!(entries[0].sequence + 1, entries[1].sequence);
    assert_eq!(entries[1].sequence + 1, entries[2].sequence);
    assert_eq!(entries[1].index_type, "lsm");
    assert_eq!(entries[2].index_type, "bt");
    assert_eq!(entries[1].rid, Some(rid));
    assert_eq!(entries[2].rid, Some(rid));
}

#[test]
fn storage_engine_stats_report_per_index_counts() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");

    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();
    engine
        .register_index(
            "bt",
            "default",
            Box::new(BTreeIndex::open("bt", &index_dir).unwrap()),
        )
        .unwrap();
    engine.set_compaction_budget_bytes(64 * 1024 * 1024);
    engine
        .put(
            b"stats-payload",
            &[
                IndexInsert::new("kv", b"k".to_vec()),
                IndexInsert::new("bt", b"p".to_vec()),
            ],
        )
        .unwrap();

    let stats = engine.stats();
    assert_eq!(stats.live_records, 1);
    assert_eq!(stats.compaction_budget_bytes, 64 * 1024 * 1024);
    assert_eq!(stats.index_stats.get("kv").unwrap().live_keys, 1);
    assert_eq!(stats.index_stats.get("bt").unwrap().live_keys, 1);
}

#[test]
fn storage_engine_compact_indexes_reports_version_reduction() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");

    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();

    let rid1 = engine.put(b"v1", &[]).unwrap();
    let rid2 = engine.put(b"v2", &[]).unwrap();
    engine.put_index("kv", b"hot-key", rid1).unwrap();
    engine.put_index("kv", b"hot-key", rid2).unwrap();
    engine.delete_index("kv", b"hot-key").unwrap();

    let before = engine.stats();
    assert!(
        before.index_stats.get("kv").unwrap().versions >= 3,
        "expected multiple versions before compaction"
    );

    let reports = engine.compact_indexes().unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].index_type, "kv");
    assert!(reports[0].after.versions <= reports[0].before.versions);

    let after = engine.stats();
    assert_eq!(after.index_compaction_runs.get("kv"), Some(&1));
    assert_eq!(after.index_stats.get("kv").unwrap().live_keys, 0);
}

#[test]
fn storage_engine_compact_selected_indexes_errors_on_unknown() {
    let dir = tempdir().unwrap();
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    let err = engine
        .compact_selected_indexes(&["does-not-exist"])
        .unwrap_err();
    assert!(matches!(err, trident::TridentError::InvalidConfig(_)));
}

#[test]
fn storage_engine_suggests_index_compactions_from_version_pressure() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();

    let r1 = engine.put(b"a", &[]).unwrap();
    let r2 = engine.put(b"b", &[]).unwrap();
    engine.put_index("kv", b"hot", r1).unwrap();
    engine.put_index("kv", b"hot", r2).unwrap();
    engine.delete_index("kv", b"hot").unwrap();

    let jobs = engine.suggest_index_compactions(2);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].index_type, "kv");
    assert!(jobs[0].estimated_versions_pruned >= 2);
}

#[test]
fn storage_engine_put_rejects_unknown_index_before_wal_append() {
    let dir = tempdir().unwrap();
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    let err = engine
        .put(b"value", &[IndexInsert::new("missing", b"k".to_vec())])
        .unwrap_err();
    assert!(matches!(err, trident::TridentError::InvalidConfig(_)));

    let entries = StorageWal::replay(&dir.path().join("wal").join("storage.wal")).unwrap();
    assert!(
        entries.is_empty(),
        "strict prevalidation must prevent WAL writes for unknown indexes"
    );
}

#[test]
fn wal_replay_preserves_delete_sequence_for_snapshot_reads() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");

    let rid = {
        let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
        engine
            .register_index(
                "kv",
                "default",
                Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
            )
            .unwrap();
        let rid = engine.put(b"snapshot-value", &[]).unwrap();
        engine.put_index("kv", b"k", rid).unwrap();
        engine.delete_index("kv", b"k").unwrap();
        rid
    };

    let entries = StorageWal::replay(&dir.path().join("wal").join("storage.wal")).unwrap();
    let put_seq = entries
        .iter()
        .find(|e| e.index_type == "kv" && e.key == b"k".to_vec() && e.rid == Some(rid))
        .unwrap()
        .sequence;
    let delete_seq = entries
        .iter()
        .find(|e| e.index_type == "kv" && e.key == b"k".to_vec() && e.rid.is_none())
        .unwrap()
        .sequence;
    assert!(delete_seq > put_seq);

    let mut reopened = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    reopened
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();
    assert_eq!(reopened.lookup_rid("kv", b"k").unwrap(), None);
    assert_eq!(
        reopened.lookup_rid_at("kv", b"k", put_seq).unwrap(),
        Some(rid)
    );
    assert_eq!(
        reopened.lookup_rid_at("kv", b"k", delete_seq).unwrap(),
        None
    );
}

#[test]
fn maintenance_cycle_suggests_and_executes_compaction_end_to_end() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();

    let r1 = engine.put(b"a", &[]).unwrap();
    let r2 = engine.put(b"b", &[]).unwrap();
    engine.put_index("kv", b"hot", r1).unwrap();
    engine.put_index("kv", b"hot", r2).unwrap();
    engine.delete_index("kv", b"hot").unwrap();

    let cycle = engine.run_maintenance_cycle(2).unwrap();
    assert_eq!(cycle.suggested.len(), 1);
    assert_eq!(cycle.suggested[0].index_type, "kv");
    assert_eq!(cycle.executed.len(), 1);
    assert_eq!(cycle.executed[0].index_type, "kv");
    let stats = engine.stats();
    assert_eq!(stats.index_compaction_runs.get("kv"), Some(&1));
}

#[test]
fn put_index_uses_wal_sequence_for_immediate_snapshot_reads() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();

    let rid = engine.put(b"value", &[]).unwrap();
    engine.put_index("kv", b"k", rid).unwrap();
    let entries = StorageWal::replay(engine.wal_path()).unwrap();
    let sequence = entries
        .iter()
        .find(|entry| {
            entry.index_type == "kv" && entry.key == b"k".to_vec() && entry.rid == Some(rid)
        })
        .unwrap()
        .sequence;
    assert_eq!(
        engine.lookup_rid_at("kv", b"k", sequence).unwrap(),
        Some(rid)
    );
}

#[test]
fn primary_only_wal_entries_are_not_counted_as_pending_index_replay() {
    let dir = tempdir().unwrap();
    {
        let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
        let _ = engine.put(b"primary-only", &[]).unwrap();
    }
    let reopened = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    assert_eq!(reopened.stats().pending_wal_replay, 0);
}

#[test]
fn maintenance_suggestions_are_deterministically_ordered() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();
    engine
        .register_index(
            "bt",
            "default",
            Box::new(BTreeIndex::open("bt", &index_dir).unwrap()),
        )
        .unwrap();

    let r1 = engine.put(b"a", &[]).unwrap();
    let r2 = engine.put(b"b", &[]).unwrap();
    // Both indexes receive the same stale-version pressure (1 stale version).
    engine.put_index("kv", b"hot", r1).unwrap();
    engine.put_index("kv", b"hot", r2).unwrap();
    engine.put_index("bt", b"hot", r1).unwrap();
    engine.put_index("bt", b"hot", r2).unwrap();

    let suggestions = engine.suggest_index_compactions(1);
    assert_eq!(suggestions.len(), 2);
    assert_eq!(
        suggestions
            .iter()
            .map(|job| job.index_type.as_str())
            .collect::<Vec<_>>(),
        vec!["bt", "kv"],
        "tie-break order must be stable and lexicographic by index type"
    );
}

#[test]
fn maintenance_cycle_with_options_respects_max_jobs() {
    let dir = tempdir().unwrap();
    let index_dir = dir.path().join("indexes");
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    engine
        .register_index(
            "kv",
            "default",
            Box::new(LsmIndex::open("kv", &index_dir).unwrap()),
        )
        .unwrap();
    engine
        .register_index(
            "bt",
            "default",
            Box::new(BTreeIndex::open("bt", &index_dir).unwrap()),
        )
        .unwrap();

    let r1 = engine.put(b"a", &[]).unwrap();
    let r2 = engine.put(b"b", &[]).unwrap();
    engine.put_index("kv", b"hot", r1).unwrap();
    engine.put_index("kv", b"hot", r2).unwrap();
    engine.put_index("bt", b"hot", r1).unwrap();
    engine.put_index("bt", b"hot", r2).unwrap();

    let report = engine
        .run_maintenance_cycle_with_options(MaintenanceCycleOptions {
            stale_version_threshold: 1,
            max_jobs: 1,
        })
        .unwrap();
    assert_eq!(report.suggested.len(), 2);
    assert_eq!(report.executed.len(), 1);

    let stats = engine.stats();
    assert_eq!(stats.maintenance_cycles_run, 1);
    assert!(stats.last_maintenance_at_sequence.is_some());
}
