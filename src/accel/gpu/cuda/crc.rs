use crate::io::crc32c;

pub fn crc32c_batch(buffers: &[&[u8]]) -> Vec<u32> {
    buffers.iter().map(|bytes| crc32c(bytes)).collect()
}
