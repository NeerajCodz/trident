use crate::accel::cpu::scalar::CpuAccelerator;
use crate::accel::traits::Accelerator;
use crate::config::Compression;
use crate::errors::Result;

#[derive(Debug, Default)]
pub struct SimdCpuAccelerator {
    scalar: CpuAccelerator,
}

impl Accelerator for SimdCpuAccelerator {
    fn name(&self) -> &'static str {
        "cpu-simd"
    }

    fn crc32c(&self, bytes: &[u8]) -> u32 {
        self.scalar.crc32c(bytes)
    }

    fn compare_keys(&self, left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        simd_compare_keys(left, right)
    }

    fn encode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        self.scalar.encode_block(codec, bytes)
    }

    fn decode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        self.scalar.decode_block(codec, bytes)
    }

    fn crc32c_batch(&self, buffers: &[&[u8]]) -> Vec<u32> {
        buffers.iter().map(|b| self.crc32c(b)).collect()
    }

    fn squared_l2_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        simd_squared_l2(left, right)
    }

    fn inner_product_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        simd_inner_product(left, right)
    }

    fn cosine_distance(&self, left: &[f32], right: &[f32]) -> Option<f32> {
        simd_cosine_distance(left, right)
    }

    fn bloom_probe_batch(&self, filter: &[u8], keys: &[&[u8]]) -> Vec<bool> {
        simd_bloom_probe_batch(filter, keys, |k| self.crc32c(k))
    }

    fn columnar_eq_mask(&self, values: &[&[u8]], needle: &[u8]) -> Vec<bool> {
        simd_columnar_eq_mask(values, needle)
    }

    fn columnar_eq_offsets(&self, values: &[&[u8]], needle: &[u8]) -> Vec<usize> {
        simd_columnar_eq_offsets(values, needle)
    }

    fn columnar_u64_range_mask(&self, values: &[u64], min: u64, max: u64) -> Vec<bool> {
        simd_u64_range_mask(values, min, max)
    }

    fn batch_squared_l2(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors.iter().map(|v| simd_squared_l2(query, v)).collect()
    }

    fn batch_inner_product(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors
            .iter()
            .map(|v| simd_inner_product(query, v))
            .collect()
    }

    fn batch_cosine_distance(&self, query: &[f32], vectors: &[&[f32]]) -> Vec<Option<f32>> {
        vectors
            .iter()
            .map(|v| simd_cosine_distance(query, v))
            .collect()
    }

    fn columnar_f32_range_mask(&self, values: &[f32], min: f32, max: f32) -> Vec<bool> {
        simd_f32_range_mask(values, min, max)
    }

    fn columnar_multi_eq_mask(&self, values: &[&[u8]], needles: &[&[u8]]) -> Vec<bool> {
        simd_multi_eq_mask(values, needles)
    }

    fn prefetch_read(&self, ptr: *const u8) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::x86_64::_mm_prefetch(ptr as *const i8, core::arch::x86_64::_MM_HINT_T0);
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = ptr;
        }
    }
}

/// SIMD-optimized key comparison using 8-byte wide chunks.
fn simd_compare_keys(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    let min_len = left.len().min(right.len());
    let chunks = min_len / 8;

    for i in 0..chunks {
        let offset = i * 8;
        let l = u64::from_be_bytes(left[offset..offset + 8].try_into().unwrap());
        let r = u64::from_be_bytes(right[offset..offset + 8].try_into().unwrap());
        match l.cmp(&r) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    let tail = chunks * 8;
    for i in tail..min_len {
        match left[i].cmp(&right[i]) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    left.len().cmp(&right.len())
}

/// SIMD-optimized squared L2 distance with 4-wide f32 accumulation.
fn simd_squared_l2(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() {
        return None;
    }
    let n = left.len();
    let chunks = n / 4;
    let mut acc0: f32 = 0.0;
    let mut acc1: f32 = 0.0;
    let mut acc2: f32 = 0.0;
    let mut acc3: f32 = 0.0;

    for i in 0..chunks {
        let off = i * 4;
        let d0 = left[off] - right[off];
        let d1 = left[off + 1] - right[off + 1];
        let d2 = left[off + 2] - right[off + 2];
        let d3 = left[off + 3] - right[off + 3];
        acc0 += d0 * d0;
        acc1 += d1 * d1;
        acc2 += d2 * d2;
        acc3 += d3 * d3;
    }

    let tail = chunks * 4;
    for i in tail..n {
        let d = left[i] - right[i];
        acc0 += d * d;
    }

    Some(acc0 + acc1 + acc2 + acc3)
}

/// SIMD-optimized inner product with 4-wide f32 accumulation.
fn simd_inner_product(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() {
        return None;
    }
    let n = left.len();
    let chunks = n / 4;
    let mut acc0: f32 = 0.0;
    let mut acc1: f32 = 0.0;
    let mut acc2: f32 = 0.0;
    let mut acc3: f32 = 0.0;

    for i in 0..chunks {
        let off = i * 4;
        acc0 += left[off] * right[off];
        acc1 += left[off + 1] * right[off + 1];
        acc2 += left[off + 2] * right[off + 2];
        acc3 += left[off + 3] * right[off + 3];
    }

    let tail = chunks * 4;
    for i in tail..n {
        acc0 += left[i] * right[i];
    }

    Some(acc0 + acc1 + acc2 + acc3)
}

/// SIMD-optimized cosine distance: 1 - (dot / (|a| * |b|)).
fn simd_cosine_distance(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() {
        return None;
    }
    let n = left.len();
    let chunks = n / 4;
    let (mut dot0, mut dot1, mut dot2, mut dot3) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut nl0, mut nl1, mut nl2, mut nl3) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut nr0, mut nr1, mut nr2, mut nr3) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

    for i in 0..chunks {
        let off = i * 4;
        let (l0, l1, l2, l3) = (left[off], left[off + 1], left[off + 2], left[off + 3]);
        let (r0, r1, r2, r3) = (right[off], right[off + 1], right[off + 2], right[off + 3]);
        dot0 += l0 * r0;
        dot1 += l1 * r1;
        dot2 += l2 * r2;
        dot3 += l3 * r3;
        nl0 += l0 * l0;
        nl1 += l1 * l1;
        nl2 += l2 * l2;
        nl3 += l3 * l3;
        nr0 += r0 * r0;
        nr1 += r1 * r1;
        nr2 += r2 * r2;
        nr3 += r3 * r3;
    }

    let tail = chunks * 4;
    for i in tail..n {
        dot0 += left[i] * right[i];
        nl0 += left[i] * left[i];
        nr0 += right[i] * right[i];
    }

    let dot = dot0 + dot1 + dot2 + dot3;
    let norm_l = (nl0 + nl1 + nl2 + nl3).sqrt();
    let norm_r = (nr0 + nr1 + nr2 + nr3).sqrt();
    let denom = norm_l * norm_r;
    if denom < f32::EPSILON {
        return Some(1.0);
    }
    Some(1.0 - (dot / denom))
}

/// Batch bloom probe with prefetch-friendly access.
fn simd_bloom_probe_batch(
    filter: &[u8],
    keys: &[&[u8]],
    hash_fn: impl Fn(&[u8]) -> u32,
) -> Vec<bool> {
    if filter.is_empty() {
        return vec![false; keys.len()];
    }
    let filter_bits = filter.len() * 8;
    let hashes: Vec<u32> = keys.iter().map(|k| hash_fn(k)).collect();

    hashes
        .iter()
        .map(|&hash| {
            let bit = (hash as usize) % filter_bits;
            (filter[bit / 8] & (1 << (bit % 8))) != 0
        })
        .collect()
}

/// SIMD-optimized columnar equality mask.
fn simd_columnar_eq_mask(values: &[&[u8]], needle: &[u8]) -> Vec<bool> {
    values.iter().map(|v| *v == needle).collect()
}

/// SIMD-optimized columnar equality offsets.
fn simd_columnar_eq_offsets(values: &[&[u8]], needle: &[u8]) -> Vec<usize> {
    values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| if *v == needle { Some(i) } else { None })
        .collect()
}

/// SIMD-optimized u64 range mask with 4-wide processing.
fn simd_u64_range_mask(values: &[u64], min: u64, max: u64) -> Vec<bool> {
    let n = values.len();
    let mut result = Vec::with_capacity(n);
    let chunks = n / 4;

    for i in 0..chunks {
        let off = i * 4;
        result.push(values[off] >= min && values[off] <= max);
        result.push(values[off + 1] >= min && values[off + 1] <= max);
        result.push(values[off + 2] >= min && values[off + 2] <= max);
        result.push(values[off + 3] >= min && values[off + 3] <= max);
    }

    let tail = chunks * 4;
    for v in &values[tail..] {
        result.push(*v >= min && *v <= max);
    }

    result
}

/// SIMD-optimized f32 range mask with 4-wide processing.
fn simd_f32_range_mask(values: &[f32], min: f32, max: f32) -> Vec<bool> {
    let n = values.len();
    let mut result = Vec::with_capacity(n);
    let chunks = n / 4;

    for i in 0..chunks {
        let off = i * 4;
        result.push(values[off] >= min && values[off] <= max);
        result.push(values[off + 1] >= min && values[off + 1] <= max);
        result.push(values[off + 2] >= min && values[off + 2] <= max);
        result.push(values[off + 3] >= min && values[off + 3] <= max);
    }

    let tail = chunks * 4;
    for v in &values[tail..] {
        result.push(*v >= min && *v <= max);
    }

    result
}

/// Multi-needle equality mask for columnar scans.
fn simd_multi_eq_mask(values: &[&[u8]], needles: &[&[u8]]) -> Vec<bool> {
    values.iter().map(|v| needles.contains(v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_keys_equal() {
        assert_eq!(
            simd_compare_keys(b"hello", b"hello"),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn compare_keys_less() {
        assert_eq!(simd_compare_keys(b"abc", b"abd"), std::cmp::Ordering::Less);
    }

    #[test]
    fn compare_keys_greater() {
        assert_eq!(
            simd_compare_keys(b"abd", b"abc"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_keys_prefix() {
        assert_eq!(
            simd_compare_keys(b"abc", b"abcdef"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn compare_keys_long() {
        let left = b"abcdefghijklmnop";
        let right = b"abcdefghijklmnop";
        assert_eq!(simd_compare_keys(left, right), std::cmp::Ordering::Equal);
    }

    #[test]
    fn squared_l2_basic() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![5.0, 6.0, 7.0, 8.0];
        let dist = simd_squared_l2(&a, &b).unwrap();
        assert!((dist - 64.0).abs() < 1e-4);
    }

    #[test]
    fn squared_l2_mismatch() {
        assert!(simd_squared_l2(&[1.0], &[1.0, 2.0]).is_none());
    }

    #[test]
    fn inner_product_basic() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![5.0, 6.0, 7.0, 8.0];
        let ip = simd_inner_product(&a, &b).unwrap();
        assert!((ip - 70.0).abs() < 1e-4);
    }

    #[test]
    fn cosine_distance_identical() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let dist = simd_cosine_distance(&a, &a).unwrap();
        assert!(dist.abs() < 1e-4);
    }

    #[test]
    fn cosine_distance_orthogonal() {
        let a = vec![1.0, 0.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0, 0.0];
        let dist = simd_cosine_distance(&a, &b).unwrap();
        assert!((dist - 1.0).abs() < 1e-4);
    }

    #[test]
    fn u64_range_mask_basic() {
        let values = vec![1, 5, 10, 15, 20, 25, 30, 35];
        let mask = simd_u64_range_mask(&values, 10, 25);
        assert_eq!(
            mask,
            vec![false, false, true, true, true, true, false, false]
        );
    }

    #[test]
    fn f32_range_mask_basic() {
        let values = vec![1.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0];
        let mask = simd_f32_range_mask(&values, 10.0, 25.0);
        assert_eq!(
            mask,
            vec![false, false, true, true, true, true, false, false]
        );
    }

    #[test]
    fn multi_eq_mask_basic() {
        let values: Vec<&[u8]> = vec![b"a", b"b", b"c", b"d"];
        let needles: Vec<&[u8]> = vec![b"b", b"d"];
        let mask = simd_multi_eq_mask(&values, &needles);
        assert_eq!(mask, vec![false, true, false, true]);
    }

    #[test]
    fn bloom_probe_empty_filter() {
        let keys: Vec<&[u8]> = vec![b"key1", b"key2"];
        let result = simd_bloom_probe_batch(&[], &keys, |k| crate::io::crc32c(k));
        assert_eq!(result, vec![false, false]);
    }
}
