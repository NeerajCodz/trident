use tempfile::tempdir;
use trident::storage::lsm::sstable::{SstableOptions, SstableReader, SstableWriter};
use trident::store::RecordId;

#[test]
fn sstable_roundtrips_sorted_point_and_range_reads() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("000001.sst");
    let mut writer = SstableWriter::create(
        &path,
        SstableOptions {
            level: 1,
            generation: 42,
            block_target_bytes: 128,
        },
    );
    writer.add_put(b"user:003".to_vec(), 3, RecordId(3));
    writer.add_put(b"user:001".to_vec(), 1, RecordId(1));
    writer.add_put(b"user:002".to_vec(), 2, RecordId(2));
    let metadata = writer.finish().unwrap();

    assert_eq!(metadata.level, 1);
    assert_eq!(metadata.generation, 42);
    assert_eq!(metadata.entry_count, 3);

    let reader = SstableReader::open(&path).unwrap();
    assert_eq!(
        reader.metadata().min_key.as_deref(),
        Some(b"user:001".as_slice())
    );
    assert_eq!(
        reader.metadata().max_key.as_deref(),
        Some(b"user:003".as_slice())
    );
    assert_eq!(
        reader.get_at(b"user:002", u64::MAX).unwrap(),
        Some(RecordId(2))
    );
    assert_eq!(reader.get_at(b"user:404", u64::MAX).unwrap(), None);

    let rows = reader.scan(Some(b"user:001"), Some(b"user:003")).unwrap();
    assert_eq!(
        rows,
        vec![
            (b"user:001".to_vec(), RecordId(1)),
            (b"user:002".to_vec(), RecordId(2))
        ]
    );
}

#[test]
fn sstable_respects_snapshot_tombstones() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("000002.sst");
    let mut writer = SstableWriter::create(&path, SstableOptions::default());
    writer.add_put(b"k".to_vec(), 10, RecordId(10));
    writer.add_tombstone(b"k".to_vec(), 20);
    writer.finish().unwrap();

    let reader = SstableReader::open(&path).unwrap();
    assert_eq!(reader.get_at(b"k", 10).unwrap(), Some(RecordId(10)));
    assert_eq!(reader.get_at(b"k", 20).unwrap(), None);
}

#[test]
fn sstable_rejects_corrupted_block_payload() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("000003.sst");
    let mut writer = SstableWriter::create(&path, SstableOptions::default());
    writer.add_put(b"k".to_vec(), 1, RecordId(1));
    writer.finish().unwrap();

    let mut bytes = std::fs::read(&path).unwrap();
    bytes[8] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();

    let reader = SstableReader::open(&path).unwrap();
    assert!(reader.get_at(b"k", u64::MAX).is_err());
}
