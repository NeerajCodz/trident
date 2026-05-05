use crate::config::Compression;
use crate::errors::Result;

pub fn encode_block(codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
    match codec {
        Compression::None => Ok(bytes.to_vec()),
        Compression::Lz4 => Ok(lz4_flex::compress_prepend_size(bytes)),
        Compression::Zstd => Ok(zstd::bulk::compress(bytes, 3)?),
    }
}

pub fn decode_block(codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
    match codec {
        Compression::None => Ok(bytes.to_vec()),
        Compression::Lz4 => Ok(lz4_flex::decompress_size_prepended(bytes)?),
        Compression::Zstd => Ok(zstd::bulk::decompress(bytes, 128 * 1024 * 1024)?),
    }
}

impl From<lz4_flex::block::DecompressError> for crate::errors::TridentError {
    fn from(value: lz4_flex::block::DecompressError) -> Self {
        Self::InvalidConfig(format!("lz4 decompression failed: {value}"))
    }
}
