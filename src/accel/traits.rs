use crate::config::Compression;
use crate::errors::Result;

pub trait Accelerator: Send + Sync {
    fn name(&self) -> &'static str;
    fn crc32c(&self, bytes: &[u8]) -> u32;
    fn compare_keys(&self, left: &[u8], right: &[u8]) -> std::cmp::Ordering;
    fn encode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>>;
    fn decode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>>;

    fn crc32c_batch(&self, buffers: &[&[u8]]) -> Vec<u32> {
        buffers.iter().map(|bytes| self.crc32c(bytes)).collect()
    }

    fn encode_blocks(&self, codec: Compression, buffers: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        buffers
            .iter()
            .map(|bytes| self.encode_block(codec, bytes))
            .collect()
    }

    fn decode_blocks(&self, codec: Compression, buffers: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
        buffers
            .iter()
            .map(|bytes| self.decode_block(codec, bytes))
            .collect()
    }

    fn squared_l2_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        crate::accel::gpu::kernels::squared_l2_distance(left, right)
    }

    fn bloom_probe_batch(&self, filter: &[u8], keys: &[&[u8]]) -> Vec<bool> {
        if filter.is_empty() {
            return vec![false; keys.len()];
        }
        keys.iter()
            .map(|key| {
                let hash = self.crc32c(key) as usize;
                let bit = hash % (filter.len() * 8);
                (filter[bit / 8] & (1 << (bit % 8))) != 0
            })
            .collect()
    }
}
