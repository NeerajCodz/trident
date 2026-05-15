use super::{
    BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope, read_bytes, read_u32,
};
use crate::errors::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterBlock {
    pub min_key: Vec<u8>,
    pub max_key: Vec<u8>,
    pub bloom_bits: Vec<u8>,
}

pub struct FilterCodec;

impl FormatCodec<FilterBlock> for FilterCodec {
    const KIND: BinaryFormatKind = BinaryFormatKind::Filter;
    const VERSION: u16 = 1;

    fn encode(value: &FilterBlock) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        for item in [&value.min_key, &value.max_key, &value.bloom_bits] {
            payload.extend_from_slice(&(item.len() as u32).to_le_bytes());
            payload.extend_from_slice(item);
        }
        Ok(encode_envelope(Self::KIND, Self::VERSION, &payload))
    }

    fn decode(bytes: &[u8]) -> Result<FilterBlock> {
        let payload = decode_envelope(bytes, Self::KIND, Self::VERSION)?;
        let mut offset = 0;
        let min_len = read_u32(payload, &mut offset, "filter min key length")? as usize;
        let min_key = read_bytes(payload, &mut offset, min_len, "filter min key")?;
        let max_len = read_u32(payload, &mut offset, "filter max key length")? as usize;
        let max_key = read_bytes(payload, &mut offset, max_len, "filter max key")?;
        let bloom_len = read_u32(payload, &mut offset, "filter bloom length")? as usize;
        let bloom_bits = read_bytes(payload, &mut offset, bloom_len, "filter bloom bits")?;
        if offset != payload.len() {
            return Err(super::corrupt("filter trailing bytes"));
        }
        Ok(FilterBlock {
            min_key,
            max_key,
            bloom_bits,
        })
    }
}
