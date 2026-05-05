//! Tests for Trident's no-duplication storage engine.
//!
//! The central invariant under test: **every value payload is written to the
//! primary [`RecordStore`] exactly once**.  All index plugins ([`LsmIndex`],
//! [`BTreeIndex`], [`AdjacencyIndex`], [`HnswIndex`]) store only
//! `key → RecordId` pointers and never duplicate the underlying bytes.

use tempfile::tempdir;
use trident::index::{AdjacencyIndex, BTreeIndex, HnswIndex, IndexPlugin, LsmIndex};
use trident::store::{RecordId, RecordStore};

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
    assert!(store.get(dead).is_err(), "deleted record must not be readable");
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
