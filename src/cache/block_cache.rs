use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

#[derive(Debug)]
pub struct BlockCache<K> {
    capacity_bytes: usize,
    current_bytes: usize,
    order: VecDeque<K>,
    entries: HashMap<K, Bytes>,
}

impl<K> BlockCache<K>
where
    K: Clone + Eq + Hash,
{
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            current_bytes: 0,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<Bytes> {
        let value = self.entries.get(key).cloned();
        if value.is_some() {
            self.order.push_back(key.clone());
        }
        value
    }

    pub fn insert(&mut self, key: K, value: Bytes) {
        if let Some(old) = self.entries.insert(key.clone(), value.clone()) {
            self.current_bytes = self.current_bytes.saturating_sub(old.len());
        }
        self.current_bytes += value.len();
        self.order.push_back(key);
        self.evict();
    }

    fn evict(&mut self) {
        while self.current_bytes > self.capacity_bytes {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            if let Some(value) = self.entries.remove(&key) {
                self.current_bytes = self.current_bytes.saturating_sub(value.len());
            }
        }
    }
}
