use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BloomFilter {
    bits: Vec<u64>,
    hash_count: u8,
    bit_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PartitionedBloomFilter {
    pub partitions: Vec<BloomFilter>,
    pub prefix_len: usize,
}

impl BloomFilter {
    pub fn with_expected_items(items: usize) -> Self {
        let bit_count = ((items.max(1) * 12).next_power_of_two()) as u64;
        let words = bit_count.div_ceil(64) as usize;
        Self {
            bits: vec![0; words],
            hash_count: 7,
            bit_count,
        }
    }

    pub fn insert(&mut self, key: &[u8]) {
        let (h1, h2) = hashes(key);
        for i in 0..self.hash_count {
            let bit = h1.wrapping_add((i as u64).wrapping_mul(h2)) % self.bit_count;
            self.bits[(bit / 64) as usize] |= 1_u64 << (bit % 64);
        }
    }

    pub fn may_contain(&self, key: &[u8]) -> bool {
        let (h1, h2) = hashes(key);
        for i in 0..self.hash_count {
            let bit = h1.wrapping_add((i as u64).wrapping_mul(h2)) % self.bit_count;
            if self.bits[(bit / 64) as usize] & (1_u64 << (bit % 64)) == 0 {
                return false;
            }
        }
        true
    }
}

impl PartitionedBloomFilter {
    pub fn new(prefix_len: usize, expected_items: usize, partition_count: usize) -> Self {
        let count = partition_count.max(1);
        let mut partitions = Vec::with_capacity(count);
        for _ in 0..count {
            partitions.push(BloomFilter::with_expected_items(expected_items / count + 1));
        }
        Self {
            partitions,
            prefix_len,
        }
    }

    pub fn insert(&mut self, key: &[u8]) {
        let partition = self.partition_for_key(key);
        self.partitions[partition].insert(key);
    }

    pub fn may_contain(&self, key: &[u8]) -> bool {
        let partition = self.partition_for_key(key);
        self.partitions[partition].may_contain(key)
    }

    fn partition_for_key(&self, key: &[u8]) -> usize {
        if self.partitions.len() == 1 {
            return 0;
        }
        let prefix = &key[..key.len().min(self.prefix_len.max(1))];
        let hash = blake3::hash(prefix);
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&hash.as_bytes()[..8]);
        (u64::from_le_bytes(bytes) as usize) % self.partitions.len()
    }
}

pub fn bloom_key(cf: &str, key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(cf.len() + 1 + key.len());
    out.extend_from_slice(cf.as_bytes());
    out.push(0);
    out.extend_from_slice(key);
    out
}

fn hashes(key: &[u8]) -> (u64, u64) {
    let hash = blake3::hash(key);
    let bytes = hash.as_bytes();
    let mut left = [0_u8; 8];
    let mut right = [0_u8; 8];
    left.copy_from_slice(&bytes[..8]);
    right.copy_from_slice(&bytes[8..16]);
    let h1 = u64::from_le_bytes(left);
    let h2 = u64::from_le_bytes(right) | 1;
    (h1, h2)
}

#[cfg(test)]
mod tests {
    use super::{BloomFilter, bloom_key};

    #[test]
    fn bloom_rejects_absent_key() {
        let mut bloom = BloomFilter::with_expected_items(8);
        bloom.insert(&bloom_key("default", b"present"));
        assert!(bloom.may_contain(&bloom_key("default", b"present")));
        assert!(!bloom.may_contain(&bloom_key("default", b"absent")));
    }
}
