use super::{BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope};
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
        let mut next = || {
            let len = u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            let item = payload[offset..offset + len].to_vec();
            offset += len;
            item
        };
        Ok(FilterBlock {
            min_key: next(),
            max_key: next(),
            bloom_bits: next(),
        })
    }
}
