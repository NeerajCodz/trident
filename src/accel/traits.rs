use crate::config::Compression;
use crate::errors::Result;

pub trait Accelerator: Send + Sync {
    fn name(&self) -> &'static str;
    fn crc32c(&self, bytes: &[u8]) -> u32;
    fn compare_keys(&self, left: &[u8], right: &[u8]) -> std::cmp::Ordering;
    fn encode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>>;
    fn decode_block(&self, codec: Compression, bytes: &[u8]) -> Result<Vec<u8>>;
}
