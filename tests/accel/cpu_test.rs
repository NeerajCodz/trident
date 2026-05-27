use praxis::accel::{Accelerator, CpuAccelerator};
use praxis::config::Compression;

#[test]
fn cpu_accelerator_roundtrips_blocks() {
    let accel = CpuAccelerator;
    let input = b"praxis-cpu-accelerator-roundtrip";

    for codec in [Compression::None, Compression::Lz4, Compression::Zstd] {
        let encoded = accel.encode_block(codec, input).unwrap();
        let decoded = accel.decode_block(codec, &encoded).unwrap();
        assert_eq!(decoded, input);
    }
}

#[test]
fn cpu_accelerator_orders_keys_and_checksums() {
    let accel = CpuAccelerator;

    assert_eq!(accel.name(), "cpu-scalar");
    assert_eq!(accel.compare_keys(b"a", b"b"), std::cmp::Ordering::Less);
    assert_eq!(accel.crc32c(b"same"), accel.crc32c(b"same"));
    assert_ne!(accel.crc32c(b"left"), accel.crc32c(b"right"));
}

#[test]
fn cpu_accelerator_filters_columnar_predicates() {
    let accel = CpuAccelerator;
    let values: Vec<&[u8]> = vec![b"open", b"closed", b"open", b"pending"];

    assert_eq!(
        accel.columnar_eq_mask(&values, b"open"),
        vec![true, false, true, false]
    );
    assert_eq!(accel.columnar_eq_offsets(&values, b"open"), vec![0, 2]);
    assert_eq!(
        accel.columnar_u64_range_mask(&[1, 5, 10, 20], 5, 10),
        vec![false, true, true, false]
    );
}
