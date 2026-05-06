use crate::accel::traits::Accelerator;
use crate::config::Compression;
use crate::errors::Result;
use crate::io::{crc32c, decode_block, encode_block};

#[derive(Debug, Default)]
pub struct CpuAccelerator;

impl Accelerator for CpuAccelerator {
    fn name(&self) -> &'static str {
        "cpu-scalar"
    }

    fn crc32c(&self, bytes: &[u8]) -> u32 {
        crc32c(bytes)
    }

    fn compare_keys(&self, left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        left.cmp(right)
    }

    fn encode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        encode_block(codec, bytes)
    }

    fn decode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        decode_block(codec, bytes)
    }
}
