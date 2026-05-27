use bytes::Bytes;
use praxis::cache::ShardedBlockCache;
use std::sync::Arc;
use std::thread;

#[test]
fn sharded_cache_tracks_hits_misses_and_evictions() {
    let cache = ShardedBlockCache::new(16, 4);

    cache.insert("a".to_string(), Bytes::from_static(b"aaaa"));
    cache.insert("b".to_string(), Bytes::from_static(b"bbbb"));
    assert_eq!(
        cache.get(&"a".to_string()),
        Some(Bytes::from_static(b"aaaa"))
    );
    assert_eq!(cache.get(&"missing".to_string()), None);

    for idx in 0..16 {
        cache.insert(format!("k{idx}"), Bytes::from_static(b"zzzz"));
    }

    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.inserts, 18);
    assert!(stats.evictions > 0);
    assert!(stats.current_bytes <= 16);
}

#[test]
fn sharded_cache_allows_parallel_read_write_access() {
    let cache = Arc::new(ShardedBlockCache::new(4096, 8));
    let mut workers = Vec::new();

    for worker in 0..8 {
        let cache = Arc::clone(&cache);
        workers.push(thread::spawn(move || {
            for item in 0..64 {
                let key = format!("{worker}:{item}");
                cache.insert(key.clone(), Bytes::from(vec![worker as u8; 8]));
                assert!(cache.get(&key).is_some());
            }
        }));
    }

    for worker in workers {
        worker.join().unwrap();
    }

    let stats = cache.stats();
    assert_eq!(cache.shard_count(), 8);
    assert_eq!(stats.inserts, 512);
    assert!(stats.hits >= 512);
}
