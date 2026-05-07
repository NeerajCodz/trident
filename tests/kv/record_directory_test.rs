use tempfile::tempdir;
use trident::index::{BTreeIndex, IndexPlugin, IndexStorageLayout};
use trident::kernel::StorageKernel;
use trident::storage::lsm::LsmIndex;
use trident::store::{RecordDirectory, RecordStore, StorageEngine};

#[test]
fn record_directory_is_the_only_recordid_resolver() {
    let dir = tempdir().unwrap();
    let mut store = RecordStore::open(dir.path()).unwrap();

    let alpha = store.put(b"alpha").unwrap();
    let beta = store.put(b"beta").unwrap();
    store.delete(beta).unwrap();

    let stats = store.canonical_stats();
    assert_eq!(stats.live_records, 1);
    assert_eq!(stats.dead_records, 1);
    assert_eq!(stats.total_records, 2);
    assert_eq!(stats.canonical_live_bytes, 5);
    assert_eq!(store.get(alpha).unwrap(), b"alpha");
    assert!(store.get(beta).is_err());
}

#[test]
fn record_directory_alias_preserves_pointer_accounting_contract() {
    let mut directory = RecordDirectory::default();
    let rid = directory.allocate(trident::store::PhysicalLocation {
        segment_id: 7,
        record_offset: 11,
        length: 19,
    });

    assert_eq!(directory.locate(rid).unwrap().segment_id, 7);
    assert_eq!(directory.live_count(), 1);
    assert_eq!(directory.live_bytes(), 19);
    assert_eq!(directory.total_count(), 1);
}

#[test]
fn built_in_indexes_declare_pointer_only_storage() {
    let dir = tempdir().unwrap();
    let lsm = LsmIndex::open("lsm", dir.path()).unwrap();
    let btree = BTreeIndex::open("btree", dir.path()).unwrap();

    assert_eq!(lsm.storage_layout(), IndexStorageLayout::POINTER_ONLY);
    assert_eq!(btree.storage_layout(), IndexStorageLayout::POINTER_ONLY);
    assert!(!lsm.storage_layout().stores_full_values);
    assert!(!btree.storage_layout().stores_full_values);
}

#[test]
fn storage_engine_implements_kernel_storage_report() {
    let dir = tempdir().unwrap();
    let mut engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();

    let rid = engine.put_record(b"canonical").unwrap();
    assert_eq!(engine.get_record(rid).unwrap(), b"canonical");

    let report = engine.storage_report();
    assert_eq!(report.live_records, 1);
    assert_eq!(report.dead_records, 0);
    assert_eq!(report.canonical_live_bytes, 9);

    engine.delete_record(rid).unwrap();
    let report = engine.storage_report();
    assert_eq!(report.live_records, 0);
    assert_eq!(report.dead_records, 1);
    assert_eq!(report.canonical_live_bytes, 0);
}
