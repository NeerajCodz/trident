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
        self.scalar.compare_keys(left, right)
    }

    fn encode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        self.scalar.encode_block(codec, bytes)
    }

    fn decode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
        self.scalar.decode_block(codec, bytes)
    }
}
