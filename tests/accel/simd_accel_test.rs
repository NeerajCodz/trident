use trident::accel::{Accelerator, CpuAccelerator};
use trident::accel::cpu::SimdCpuAccelerator;
use trident::config::Compression;

#[test]
fn simd_accelerator_matches_scalar_crc32c() {
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    for payload in [
        b"short".as_slice(),
        b"this is a medium length payload for crc testing".as_slice(),
        &vec![0xAA_u8; 65536],
    ] {
        assert_eq!(
            simd.crc32c(payload),
            scalar.crc32c(payload),
            "crc32c mismatch for payload len {}",
            payload.len(),
        );
    }
}

#[test]
fn simd_accelerator_matches_scalar_compare_keys() {
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    let pairs: Vec<(&[u8], &[u8])> = vec![
        (b"abc", b"abd"),
        (b"hello", b"hello"),
        (b"z", b"a"),
        (b"prefix", b"prefix_longer"),
        (b"abcdefghijklmnopqrstuvwxyz", b"abcdefghijklmnopqrstuvwxyz"),
        (b"", b""),
        (b"", b"a"),
    ];

    for (left, right) in pairs {
        assert_eq!(
            simd.compare_keys(left, right),
            scalar.compare_keys(left, right),
            "key compare mismatch for {:?} vs {:?}",
            left,
            right,
        );
    }
}

#[test]
fn simd_accelerator_roundtrips_blocks() {
    let simd = SimdCpuAccelerator::default();
    let input = b"trident-simd-accelerator-roundtrip-test-data";

    for codec in [Compression::None, Compression::Lz4, Compression::Zstd] {
        let encoded = simd.encode_block(codec, input).unwrap();
        let decoded = simd.decode_block(codec, &encoded).unwrap();
        assert_eq!(decoded, input);
    }
}

#[test]
fn simd_squared_l2_matches_scalar() {
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    let left: Vec<f32> = (0..128).map(|i| i as f32 * 0.1).collect();
    let right: Vec<f32> = (0..128).map(|i| (128 - i) as f32 * 0.1).collect();

    let scalar_result = scalar.squared_l2_distance(&left, &right).unwrap();
    let simd_result = simd.squared_l2_distance(&left, &right).unwrap();

    assert!(
        (scalar_result - simd_result).abs() < 1e-2,
        "L2 distance mismatch: scalar={scalar_result}, simd={simd_result}"
    );
}

#[test]
fn simd_inner_product_correctness() {
    let simd = SimdCpuAccelerator::default();

    let a = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let b = vec![8.0f32, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0];
    let ip = simd.inner_product_distance(&a, &b).unwrap();
    // 1*8 + 2*7 + 3*6 + 4*5 + 5*4 + 6*3 + 7*2 + 8*1 = 120
    assert!((ip - 120.0).abs() < 1e-4);
}

#[test]
fn simd_cosine_distance_correctness() {
    let simd = SimdCpuAccelerator::default();

    let a = vec![1.0f32, 0.0, 0.0, 0.0];
    let b = vec![0.0f32, 1.0, 0.0, 0.0];
    let dist = simd.cosine_distance(&a, &b).unwrap();
    assert!((dist - 1.0).abs() < 1e-4, "Orthogonal vectors should have cosine distance 1.0");

    let c = vec![3.0f32, 4.0, 0.0, 0.0];
    let dist_self = simd.cosine_distance(&c, &c).unwrap();
    assert!(dist_self.abs() < 1e-4, "Self cosine distance should be 0.0");
}

#[test]
fn simd_batch_squared_l2() {
    let simd = SimdCpuAccelerator::default();

    let query = vec![1.0f32, 2.0, 3.0, 4.0];
    let v1 = vec![5.0f32, 6.0, 7.0, 8.0];
    let v2 = vec![1.0f32, 2.0, 3.0, 4.0];
    let v3 = vec![0.0f32, 0.0, 0.0, 0.0];

    let vectors: Vec<&[f32]> = vec![&v1, &v2, &v3];
    let results = simd.batch_squared_l2(&query, &vectors);

    assert_eq!(results.len(), 3);
    assert!((results[0].unwrap() - 64.0).abs() < 1e-4);
    assert!((results[1].unwrap() - 0.0).abs() < 1e-4);
    assert!((results[2].unwrap() - 30.0).abs() < 1e-4);
}

#[test]
fn simd_batch_inner_product() {
    let simd = SimdCpuAccelerator::default();

    let query = vec![1.0f32, 2.0, 3.0, 4.0];
    let v1 = vec![1.0f32, 0.0, 0.0, 0.0];
    let v2 = vec![0.0f32, 0.0, 0.0, 1.0];

    let vectors: Vec<&[f32]> = vec![&v1, &v2];
    let results = simd.batch_inner_product(&query, &vectors);

    assert_eq!(results.len(), 2);
    assert!((results[0].unwrap() - 1.0).abs() < 1e-4);
    assert!((results[1].unwrap() - 4.0).abs() < 1e-4);
}

#[test]
fn simd_columnar_f32_range_mask() {
    let simd = SimdCpuAccelerator::default();

    let values = vec![1.0f32, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0];
    let mask = simd.columnar_f32_range_mask(&values, 10.0, 30.0);
    assert_eq!(
        mask,
        vec![false, false, true, true, true, true, true, false, false]
    );
}

#[test]
fn simd_columnar_multi_eq_mask() {
    let simd = SimdCpuAccelerator::default();

    let values: Vec<&[u8]> = vec![b"cat", b"dog", b"bird", b"cat", b"fish"];
    let needles: Vec<&[u8]> = vec![b"cat", b"fish"];
    let mask = simd.columnar_multi_eq_mask(&values, &needles);
    assert_eq!(mask, vec![true, false, false, true, true]);
}

#[test]
fn simd_bloom_probe_batch() {
    let simd = SimdCpuAccelerator::default();

    let result = simd.bloom_probe_batch(&[], &[b"key1", b"key2"]);
    assert_eq!(result, vec![false, false]);

    let filter = vec![0xFF_u8; 64];
    let result = simd.bloom_probe_batch(&filter, &[b"key1", b"key2"]);
    assert_eq!(result, vec![true, true]);
}

#[test]
fn simd_dimension_mismatch_returns_none() {
    let simd = SimdCpuAccelerator::default();

    assert!(simd.squared_l2_distance(&[1.0], &[1.0, 2.0]).is_none());
    assert!(simd.inner_product_distance(&[1.0], &[1.0, 2.0]).is_none());
    assert!(simd.cosine_distance(&[1.0], &[1.0, 2.0]).is_none());
}

#[test]
fn simd_crc32c_batch() {
    let scalar = CpuAccelerator;
    let simd = SimdCpuAccelerator::default();

    let buffers: Vec<Vec<u8>> = (0..10)
        .map(|i| format!("buffer-{i}").into_bytes())
        .collect();
    let refs: Vec<&[u8]> = buffers.iter().map(Vec::as_slice).collect();

    let scalar_results = scalar.crc32c_batch(&refs);
    let simd_results = simd.crc32c_batch(&refs);
    assert_eq!(scalar_results, simd_results);
}
