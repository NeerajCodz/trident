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

    fn inner_product_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        if left.len() != right.len() {
            return None;
        }
        Some(left.iter().zip(right).map(|(a, b)| a * b).sum())
    }

    fn cosine_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        if left.len() != right.len() {
            return None;
        }
        let mut dot = 0.0f32;
        let mut norm_l = 0.0f32;
        let mut norm_r = 0.0f32;
        for (a, b) in left.iter().zip(right) {
            dot += a * b;
            norm_l += a * a;
            norm_r += b * b;
        }
        let denom = norm_l.sqrt() * norm_r.sqrt();
        if denom < f32::EPSILON {
            return Some(1.0);
        }
        Some(1.0 - (dot / denom))
    }

    fn batch_squared_l2(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors
            .iter()
            .map(|v| self.squared_l2_distance(query, v))
            .collect()
    }

    fn batch_inner_product(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors
            .iter()
            .map(|v| self.inner_product_distance(query, v))
            .collect()
    }

    fn batch_cosine_distance(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors
            .iter()
            .map(|v| self.cosine_distance(query, v))
            .collect()
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

    fn columnar_eq_mask(&self, values: &[&[u8]], needle: &[u8]) -> Vec<bool> {
        values.iter().map(|value| *value == needle).collect()
    }

    fn columnar_eq_offsets(&self, values: &[&[u8]], needle: &[u8]) -> Vec<usize> {
        self.columnar_eq_mask(values, needle)
            .into_iter()
            .enumerate()
            .filter_map(|(offset, matched)| matched.then_some(offset))
            .collect()
    }

    fn columnar_u64_range_mask(&self, values: &[u64], min: u64, max: u64) -> Vec<bool> {
        values
            .iter()
            .map(|value| (min..=max).contains(value))
            .collect()
    }

    fn columnar_f32_range_mask(&self, values: &[f32], min: f32, max: f32) -> Vec<bool> {
        values.iter().map(|v| *v >= min && *v <= max).collect()
    }

    fn columnar_multi_eq_mask(&self, values: &[&[u8]], needles: &[&[u8]]) -> Vec<bool> {
        values
            .iter()
            .map(|v| needles.contains(v))
            .collect()
    }

    fn prefetch_read(&self, _ptr: *const u8) {}
}
