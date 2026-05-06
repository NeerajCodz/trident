use crate::config::Compression;
use crate::errors::Result;
use crate::io::{decode_block, encode_block};

pub fn encode(codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
    encode_block(codec, bytes)
}

pub fn decode(codec: Compression, bytes: &[u8]) -> Result<Vec<u8>> {
    decode_block(codec, bytes)
}
