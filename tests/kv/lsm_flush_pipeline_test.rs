use tempfile::tempdir;
use trident::storage::lsm::{LsmFlushPipeline, MutableMemtable, sstable::SstableReader};
use trident::store::RecordId;

#[test]
fn lsm_flush_pipeline_writes_immutable_memtable_to_sstable() {
    let dir = tempdir().unwrap();
    let mut memtable = MutableMemtable::default();
    memtable.put(b"k1".to_vec(), 1, RecordId(1));
    memtable.put(b"k2".to_vec(), 2, RecordId(2));

    let mut pipeline = LsmFlushPipeline::new(dir.path(), 7);
    pipeline.enqueue(memtable.freeze());

    let report = pipeline.flush_next().unwrap().unwrap();
    let reader = SstableReader::open(&report.path).unwrap();

    assert_eq!(report.entries, 2);
    assert_eq!(report.sstable_generation, 7);
    assert_eq!(reader.get_at(b"k1", u64::MAX).unwrap(), Some(RecordId(1)));
}
