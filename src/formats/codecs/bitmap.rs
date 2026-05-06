use super::{BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope};
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
        let count = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
        let mut bits = Vec::with_capacity(count);
        for chunk in payload[4..].chunks_exact(8).take(count) {
            bits.push(u64::from_le_bytes(chunk.try_into().unwrap()));
        }
        Ok(BitmapBlock { bits })
    }
}
