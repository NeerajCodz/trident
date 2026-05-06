use super::{BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope};
use crate::errors::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowRecord {
    pub record_id: u64,
    pub bytes: Vec<u8>,
}

pub struct RowRecordCodec;

impl FormatCodec<RowRecord> for RowRecordCodec {
    const KIND: BinaryFormatKind = BinaryFormatKind::RowRecord;
    const VERSION: u16 = 1;

    fn encode(value: &RowRecord) -> Result<Vec<u8>> {
        let mut payload = Vec::with_capacity(12 + value.bytes.len());
        payload.extend_from_slice(&value.record_id.to_le_bytes());
        payload.extend_from_slice(&(value.bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(&value.bytes);
        Ok(encode_envelope(Self::KIND, Self::VERSION, &payload))
    }

    fn decode(bytes: &[u8]) -> Result<RowRecord> {
        let payload = decode_envelope(bytes, Self::KIND, Self::VERSION)?;
        if payload.len() < 12 {
            return Err(super::corrupt("row payload too short"));
        }
        let record_id = u64::from_le_bytes(payload[0..8].try_into().unwrap());
        let len = u32::from_le_bytes(payload[8..12].try_into().unwrap()) as usize;
        if payload.len() != 12 + len {
            return Err(super::corrupt("row length mismatch"));
        }
        Ok(RowRecord {
            record_id,
            bytes: payload[12..].to_vec(),
        })
    }
}
