use praxis::identity::{Aid, Cid, Eid, FieldId};
use praxis::manifest::PageManifestStore;
use praxis::record::PageRecordStore;

#[test]
fn page_record_store_tracks_page_slot_and_directory_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PageRecordStore::open(dir.path(), Cid(1), Eid(2)).unwrap();

    store
        .put(&[(FieldId::Fixed(Aid(1)), b"manifested".as_slice())])
        .unwrap();

    let manifest_path = store.layout().page_manifest_path().path;
    let manifest = PageManifestStore::open(&manifest_path).load().unwrap();
    let formats: Vec<_> = manifest
        .files
        .iter()
        .map(|file| file.format.as_str())
        .collect();

    assert!(manifest_path.exists());
    assert!(formats.contains(&"record-page"));
    assert!(formats.contains(&"slot-directory"));
    assert!(formats.contains(&"rid-directory"));
    assert!(
        manifest
            .files
            .iter()
            .all(|file| file.recoverable && !file.checksum.is_empty())
    );
}

#[test]
fn page_manifest_rejects_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PageRecordStore::open(dir.path(), Cid(1), Eid(2)).unwrap();
    store
        .put(&[(FieldId::Fixed(Aid(1)), b"manifested".as_slice())])
        .unwrap();

    let path = store.layout().page_manifest_path().path;
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();

    assert!(PageManifestStore::open(path).load().is_err());
}
