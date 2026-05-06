use super::{
    decode_envelope, encode_envelope, read_bytes, read_u32, read_u64, BinaryFormatKind, FormatCodec,
};
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
        let mut offset = 0;
        let record_id = read_u64(payload, &mut offset, "row record_id")?;
        let len = read_u32(payload, &mut offset, "row length")? as usize;
        let bytes = read_bytes(payload, &mut offset, len, "row bytes")?;
        if offset != payload.len() {
            return Err(super::corrupt("row length mismatch"));
        }
        Ok(RowRecord { record_id, bytes })
    }
}
