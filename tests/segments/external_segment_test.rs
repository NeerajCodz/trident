use praxis::datatype::SegmentFamily;
use praxis::segments::BlobStore;
use tempfile::tempdir;

#[test]
fn overflow_blob_store_appends_and_reads_bytes() {
    let dir = tempdir().unwrap();
    let store = BlobStore::open_overflow(dir.path().join("overflow.blob")).unwrap();

    let location = store.append(b"large text body").unwrap();

    assert_eq!(store.read(&location).unwrap(), b"large text body");
    assert_eq!(location.family, None);
}

#[test]
fn segment_blob_store_keeps_family_identity() {
    let dir = tempdir().unwrap();
    let vector =
        BlobStore::open_segment(dir.path().join("vector.seg"), SegmentFamily::Vector, 9).unwrap();
    let fulltext =
        BlobStore::open_segment(dir.path().join("fulltext.seg"), SegmentFamily::FullText, 9)
            .unwrap();

    let location = vector.append(&[1, 2, 3, 4]).unwrap();

    assert_eq!(vector.read(&location).unwrap(), vec![1, 2, 3, 4]);
    assert!(fulltext.read(&location).is_err());
}

#[test]
fn blob_store_rejects_corrupted_payload() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("overflow.blob");
    let store = BlobStore::open_overflow(&path).unwrap();
    let location = store.append(b"payload").unwrap();

    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();

    assert!(store.read(&location).is_err());
}
