use tempfile::tempdir;
use trident::store::RecordStore;

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
