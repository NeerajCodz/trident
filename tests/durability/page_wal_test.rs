use praxis::config::WalSyncPolicy;
use praxis::identity::{Aid, Cid, Eid, FieldId, Rid};
use praxis::record::PageRecordStore;
use praxis::wal::{PageWal, PageWalMutation, PageWalRecord};

#[test]
fn page_wal_roundtrips_put_and_delete_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".praxis").join("logs").join("page.wal");
    let mut wal = PageWal::open(&path, WalSyncPolicy::EveryBatch).unwrap();

    let put = wal
        .append_put(
            Cid(1),
            Eid(2),
            Rid(7),
            &[(FieldId::Fixed(Aid(3)), b"value".as_slice())],
        )
        .unwrap();
    let delete = wal.append_delete(Cid(1), Eid(2), Rid(7)).unwrap();

    let replayed = PageWal::replay(&path).unwrap();
    assert_eq!(replayed, vec![put, delete]);
}

#[test]
fn page_record_store_replays_wal_after_directory_loss() {
    let dir = tempfile::tempdir().unwrap();
    let cid = Cid(1);
    let eid = Eid(2);
    let mut store = PageRecordStore::open(dir.path(), cid, eid).unwrap();
    let wal_path = store.layout().logs_root().join("page.wal");
    let mut wal = PageWal::open(&wal_path, WalSyncPolicy::EveryBatch).unwrap();

    let rid = store
        .put_durable(&mut wal, &[(FieldId::Fixed(Aid(1)), b"durable".as_slice())])
        .unwrap();
    assert_eq!(rid, Rid(1));

    std::fs::remove_file(store.layout().critical_map_path(cid, "rid_to_slot").path).unwrap();
    let records = PageWal::replay(&wal_path).unwrap();
    let mut recovered = PageRecordStore::open(dir.path(), cid, eid).unwrap();
    recovered.replay_page_wal(&records).unwrap();

    assert_eq!(recovered.get(rid).unwrap(), b"durable");
}

#[test]
fn page_record_store_replays_put_when_wal_was_durable_before_page_apply() {
    let dir = tempfile::tempdir().unwrap();
    let cid = Cid(1);
    let eid = Eid(2);
    let mut store = PageRecordStore::open(dir.path(), cid, eid).unwrap();
    let wal_path = store.layout().logs_root().join("page.wal");
    let mut wal = PageWal::open(&wal_path, WalSyncPolicy::EveryBatch).unwrap();

    wal.append_put(
        cid,
        eid,
        Rid(1),
        &[(FieldId::Fixed(Aid(1)), b"after-wal".as_slice())],
    )
    .unwrap();

    let records = PageWal::replay(&wal_path).unwrap();
    store.replay_page_wal(&records).unwrap();

    assert_eq!(store.get(Rid(1)).unwrap(), b"after-wal");
}

#[test]
fn page_record_store_replays_delete_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    let cid = Cid(1);
    let eid = Eid(2);
    let mut store = PageRecordStore::open(dir.path(), cid, eid).unwrap();
    let wal_path = store.layout().logs_root().join("page.wal");
    let mut wal = PageWal::open(&wal_path, WalSyncPolicy::EveryBatch).unwrap();

    let rid = store
        .put_durable(&mut wal, &[(FieldId::Fixed(Aid(1)), b"gone".as_slice())])
        .unwrap();
    store.delete_durable(&mut wal, rid).unwrap();

    let records = PageWal::replay(&wal_path).unwrap();
    let mut recovered = PageRecordStore::open(dir.path(), cid, eid).unwrap();
    recovered.replay_page_wal(&records).unwrap();
    recovered.replay_page_wal(&records).unwrap();

    assert!(recovered.get(rid).is_err());
}

#[test]
fn page_wal_replay_ignores_torn_tail_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".praxis").join("logs").join("page.wal");
    let mut wal = PageWal::open(&path, WalSyncPolicy::EveryBatch).unwrap();
    let put = wal
        .append_put(
            Cid(1),
            Eid(2),
            Rid(1),
            &[(FieldId::Fixed(Aid(1)), b"ok".as_slice())],
        )
        .unwrap();
    let torn = PageWalRecord {
        sequence: 99,
        mutation: PageWalMutation::Delete {
            cid: Cid(1),
            eid: Eid(2),
            rid: Rid(1),
        },
    }
    .encode();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(&torn[..torn.len() / 2]);
    std::fs::write(&path, bytes).unwrap();

    assert_eq!(PageWal::replay(&path).unwrap(), vec![put]);
}
