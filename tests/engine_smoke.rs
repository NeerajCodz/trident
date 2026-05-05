use bytes::Bytes;
use tempfile::tempdir;
use trident::{TridentConfig, TridentEngine, WriteBatch};

#[test]
fn put_get_delete_and_recover_from_wal() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    engine
        .put(Bytes::from("hello"), Bytes::from("world"))
        .unwrap();
    assert_eq!(engine.get("hello").unwrap().unwrap(), Bytes::from("world"));
    drop(engine);

    let recovered = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    assert_eq!(
        recovered.get("hello").unwrap().unwrap(),
        Bytes::from("world")
    );
    recovered.delete(Bytes::from("hello")).unwrap();
    assert_eq!(recovered.get("hello").unwrap(), None);
}

#[test]
fn write_batch_is_one_sequence_and_flushes_to_segment() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    let mut batch = WriteBatch::new();
    batch.put_default(Bytes::from("a"), Bytes::from("1"));
    batch.put_default(Bytes::from("b"), Bytes::from("2"));
    let sequence = engine.write_batch(batch).unwrap();
    assert_eq!(sequence, 1);
    assert_eq!(engine.flush().unwrap(), Some(1));
    drop(engine);

    let reopened = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    assert_eq!(reopened.get("a").unwrap().unwrap(), Bytes::from("1"));
    assert_eq!(reopened.get("b").unwrap().unwrap(), Bytes::from("2"));
}

#[test]
fn scan_returns_sorted_visible_values() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    engine.put(Bytes::from("k3"), Bytes::from("v3")).unwrap();
    engine.put(Bytes::from("k1"), Bytes::from("v1")).unwrap();
    engine.put(Bytes::from("k2"), Bytes::from("v2")).unwrap();
    let rows = engine.scan(Some(b"k1"), Some(b"k3"), 10).unwrap();
    assert_eq!(
        rows,
        vec![
            (Bytes::from("k1"), Bytes::from("v1")),
            (Bytes::from("k2"), Bytes::from("v2"))
        ]
    );
}

#[test]
fn large_values_move_to_value_log_and_survive_restart() {
    let dir = tempdir().unwrap();
    let mut config = TridentConfig::new(dir.path());
    config.large_value_threshold = 8;
    let engine = TridentEngine::open(config.clone()).unwrap();
    let value = Bytes::from(vec![42_u8; 4096]);
    engine.put(Bytes::from("large"), value.clone()).unwrap();
    engine.flush().unwrap();
    drop(engine);

    let reopened = TridentEngine::open(config).unwrap();
    assert_eq!(reopened.get("large").unwrap().unwrap(), value);
}

#[test]
fn compaction_collapses_overwrites_and_tombstones() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    engine.put(Bytes::from("keep"), Bytes::from("old")).unwrap();
    engine.flush().unwrap();
    engine.put(Bytes::from("keep"), Bytes::from("new")).unwrap();
    engine
        .put(Bytes::from("gone"), Bytes::from("soon"))
        .unwrap();
    engine.flush().unwrap();
    engine.delete(Bytes::from("gone")).unwrap();
    engine.flush().unwrap();

    assert_eq!(engine.compact().unwrap(), 1);
    assert_eq!(engine.get("keep").unwrap().unwrap(), Bytes::from("new"));
    assert_eq!(engine.get("gone").unwrap(), None);

    let stats = engine.stats();
    assert_eq!(stats["manifest"]["segments"].as_array().unwrap().len(), 1);
}
