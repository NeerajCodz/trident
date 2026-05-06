use super::{BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope};
use crate::errors::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnarBlock {
    pub column_id: u32,
    pub values: Vec<Vec<u8>>,
}

pub struct ColumnarBlockCodec;

impl FormatCodec<ColumnarBlock> for ColumnarBlockCodec {
    const KIND: BinaryFormatKind = BinaryFormatKind::ColumnarBlock;
    const VERSION: u16 = 1;

    fn encode(value: &ColumnarBlock) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&value.column_id.to_le_bytes());
        payload.extend_from_slice(&(value.values.len() as u32).to_le_bytes());
        for item in &value.values {
            payload.extend_from_slice(&(item.len() as u32).to_le_bytes());
            payload.extend_from_slice(item);
        }
        Ok(encode_envelope(Self::KIND, Self::VERSION, &payload))
    }

    fn decode(bytes: &[u8]) -> Result<ColumnarBlock> {
        let payload = decode_envelope(bytes, Self::KIND, Self::VERSION)?;
        let column_id = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        let count = u32::from_le_bytes(payload[4..8].try_into().unwrap()) as usize;
        let mut offset = 8;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            values.push(payload[offset..offset + len].to_vec());
            offset += len;
        }
        Ok(ColumnarBlock { column_id, values })
    }
}
