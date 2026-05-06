use super::{
    decode_envelope, encode_envelope, read_bytes, read_u32, BinaryFormatKind, FormatCodec,
};
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
        let mut offset = 0;
        let column_id = read_u32(payload, &mut offset, "column id")?;
        let count = read_u32(payload, &mut offset, "column value count")? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let len = read_u32(payload, &mut offset, "column value length")? as usize;
            values.push(read_bytes(payload, &mut offset, len, "column value")?);
        }
        if offset != payload.len() {
            return Err(super::corrupt("columnar trailing bytes"));
        }
        Ok(ColumnarBlock { column_id, values })
    }
}
