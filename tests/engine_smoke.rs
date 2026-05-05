use bytes::Bytes;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use tempfile::tempdir;
use trident::{AsyncTridentEngine, TridentConfig, TridentEngine, TridentError, WriteBatch};

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

#[test]
fn compare_and_swap_enforces_expected_value() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    engine.put(Bytes::from("cas"), Bytes::from("v1")).unwrap();
    let err = engine
        .compare_and_swap(Bytes::from("cas"), Some(b"wrong"), Bytes::from("v2"))
        .unwrap_err();
    assert!(matches!(err, TridentError::CompareAndSwapFailed));
    engine
        .compare_and_swap(Bytes::from("cas"), Some(b"v1"), Bytes::from("v2"))
        .unwrap();
    assert_eq!(engine.get("cas").unwrap().unwrap(), Bytes::from("v2"));
}

#[test]
fn torn_wal_suffix_is_ignored_during_recovery() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    engine
        .put(Bytes::from("safe"), Bytes::from("value"))
        .unwrap();
    drop(engine);

    let wal_path = dir.path().join("wal").join("00000000000000000001.wal");
    let mut wal = OpenOptions::new().append(true).open(wal_path).unwrap();
    wal.write_all(&[0x4c, 0x41, 0x57]).unwrap();
    wal.flush().unwrap();

    let reopened = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    assert_eq!(reopened.get("safe").unwrap().unwrap(), Bytes::from("value"));
}

#[test]
fn deterministic_operation_stream_matches_btree_oracle() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    let mut oracle = BTreeMap::new();

    for i in 0..500_u32 {
        let key = format!("key/{:03}", (i * 37) % 91);
        if i % 7 == 0 {
            engine.delete(Bytes::from(key.clone())).unwrap();
            oracle.remove(&key);
        } else {
            let value = format!("value/{i:04}");
            engine
                .put(Bytes::from(key.clone()), Bytes::from(value.clone()))
                .unwrap();
            oracle.insert(key, value);
        }
        if i % 53 == 0 {
            engine.flush().unwrap();
        }
    }

    engine.compact().unwrap();
    for (key, value) in oracle {
        assert_eq!(
            engine.get(key.as_bytes()).unwrap().unwrap(),
            Bytes::from(value)
        );
    }
}

#[test]
fn checkpoint_and_gc_reclaim_obsolete_files_after_compaction() {
    let dir = tempdir().unwrap();
    let engine = TridentEngine::open(TridentConfig::new(dir.path())).unwrap();
    engine.put(Bytes::from("a"), Bytes::from("1")).unwrap();
    engine.flush().unwrap();
    engine.put(Bytes::from("a"), Bytes::from("2")).unwrap();
    engine.flush().unwrap();
    engine.compact().unwrap();

    let checkpoint = engine.checkpoint().unwrap();
    assert_eq!(checkpoint.sequence, engine.snapshot().sequence);

    let report = engine.garbage_collect().unwrap();
    assert!(report.files_reclaimed >= 1);
    assert!(report.bytes_reclaimed > 0);
}

#[tokio::test]
async fn async_engine_wraps_sync_core() {
    let dir = tempdir().unwrap();
    let engine = AsyncTridentEngine::open(TridentConfig::new(dir.path()))
        .await
        .unwrap();
    engine
        .put(Bytes::from("async-key"), Bytes::from("async-value"))
        .await
        .unwrap();
    assert_eq!(
        engine.get(Bytes::from("async-key")).await.unwrap().unwrap(),
        Bytes::from("async-value")
    );
    assert_eq!(engine.flush().await.unwrap(), Some(1));
}
