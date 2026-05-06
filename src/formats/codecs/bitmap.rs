use super::{BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope, read_u32, read_u64};
use crate::errors::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitmapBlock {
    pub bits: Vec<u64>,
}

pub struct BitmapCodec;

impl FormatCodec<BitmapBlock> for BitmapCodec {
    const KIND: BinaryFormatKind = BinaryFormatKind::Bitmap;
    const VERSION: u16 = 1;

    fn encode(value: &BitmapBlock) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(value.bits.len() as u32).to_le_bytes());
        for word in &value.bits {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        Ok(encode_envelope(Self::KIND, Self::VERSION, &payload))
    }

    fn decode(bytes: &[u8]) -> Result<BitmapBlock> {
        let payload = decode_envelope(bytes, Self::KIND, Self::VERSION)?;
        let mut offset = 0;
        let count = read_u32(payload, &mut offset, "bitmap word count")? as usize;
        let mut bits = Vec::with_capacity(count);
        for _ in 0..count {
            bits.push(read_u64(payload, &mut offset, "bitmap word")?);
        }
        if offset != payload.len() {
            return Err(super::corrupt("bitmap trailing bytes"));
        }
        Ok(BitmapBlock { bits })
    }
}
