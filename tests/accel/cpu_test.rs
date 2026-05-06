use trident::accel::{Accelerator, CpuAccelerator};
use trident::config::Compression;

#[test]
fn cpu_accelerator_roundtrips_blocks() {
    let accel = CpuAccelerator;
    let input = b"trident-cpu-accelerator-roundtrip";

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
