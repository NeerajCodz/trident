use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

#[derive(Debug)]
pub struct BlockCache<K> {
    capacity_bytes: usize,
    current_bytes: usize,
    clock: u64,
    order: VecDeque<(K, u64)>,
    entries: HashMap<K, CacheEntry>,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    value: Bytes,
    generation: u64,
}

impl<K> BlockCache<K>
where
    K: Clone + Eq + Hash,
{
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            current_bytes: 0,
            clock: 0,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<Bytes> {
        self.entries.get_mut(key).map(|entry| {
            self.clock += 1;
            entry.generation = self.clock;
            self.order.push_back((key.clone(), entry.generation));
            entry.value.clone()
        })
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    pub fn value_len(&self, key: &K) -> Option<usize> {
        self.entries.get(key).map(|entry| entry.value.len())
    }

    pub fn insert(&mut self, key: K, value: Bytes) {
        let _ = self.insert_with_evicted(key, value);
    }

    pub fn insert_with_evicted(&mut self, key: K, value: Bytes) -> Vec<(K, Bytes)> {
        let mut evicted = Vec::new();
        self.clock += 1;
        if let Some(old) = self.entries.insert(
            key.clone(),
            CacheEntry {
                value: value.clone(),
                generation: self.clock,
            },
        ) {
            self.current_bytes = self.current_bytes.saturating_sub(old.value.len());
        }
        self.current_bytes += value.len();
        self.order.push_back((key, self.clock));
        self.evict(&mut evicted);
        evicted
    }

    fn evict(&mut self, evicted: &mut Vec<(K, Bytes)>) {
        while self.current_bytes > self.capacity_bytes {
            let Some((key, generation)) = self.order.pop_front() else {
                break;
            };
            let is_current = self
                .entries
                .get(&key)
                .is_some_and(|entry| entry.generation == generation);
            if !is_current {
                continue;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.current_bytes = self.current_bytes.saturating_sub(entry.value.len());
                evicted.push((key, entry.value));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BlockCache;
    use bytes::Bytes;

    #[test]
    fn cache_does_not_evict_fresh_generation_from_stale_queue_entry() {
        let mut cache = BlockCache::new(10);
        cache.insert("a", Bytes::from_static(b"aaaa"));
        cache.insert("b", Bytes::from_static(b"bbbb"));
        assert_eq!(cache.get(&"a"), Some(Bytes::from_static(b"aaaa")));
        cache.insert("c", Bytes::from_static(b"cccc"));

        assert!(cache.contains_key(&"a"));
        assert!(!cache.contains_key(&"b"));
        assert!(cache.contains_key(&"c"));
        assert!(cache.current_bytes() <= 10);
    }
}
