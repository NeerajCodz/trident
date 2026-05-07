use super::BlockCache;
use bytes::Bytes;
use parking_lot::Mutex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub inserts: u64,
    pub evictions: u64,
    pub current_bytes: u64,
}

#[derive(Debug)]
pub struct ShardedBlockCache<K> {
    shards: Vec<Mutex<BlockCache<K>>>,
    hits: AtomicU64,
    misses: AtomicU64,
    inserts: AtomicU64,
    evictions: AtomicU64,
}

impl<K> ShardedBlockCache<K>
where
    K: Clone + Eq + Hash,
{
    pub fn new(capacity_bytes: usize, requested_shards: usize) -> Self {
        let shard_count = requested_shards.next_power_of_two().max(1);
        let per_shard = capacity_bytes.div_ceil(shard_count);
        let shards = (0..shard_count)
            .map(|_| Mutex::new(BlockCache::new(per_shard)))
            .collect();
        Self {
            shards,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            inserts: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    pub fn get(&self, key: &K) -> Option<Bytes> {
        let value = self.shard_for(key).lock().get(key);
        if value.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        value
    }

    pub fn insert(&self, key: K, value: Bytes) {
        let evicted = self.shard_for(&key).lock().insert_with_evicted(key, value);
        self.inserts.fetch_add(1, Ordering::Relaxed);
        self.evictions
            .fetch_add(evicted.len() as u64, Ordering::Relaxed);
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    pub fn current_bytes(&self) -> u64 {
        self.shards
            .iter()
            .map(|shard| shard.lock().current_bytes() as u64)
            .sum()
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            inserts: self.inserts.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            current_bytes: self.current_bytes(),
        }
    }

    fn shard_for(&self, key: &K) -> &Mutex<BlockCache<K>> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() as usize) & (self.shards.len() - 1);
        &self.shards[index]
    }
}
