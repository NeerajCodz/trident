use super::{
    decode_envelope, encode_envelope, read_bytes, read_u32, read_u64, BinaryFormatKind, FormatCodec,
};
use crate::errors::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostingsList {
    pub term: Vec<u8>,
    pub record_ids: Vec<u64>,
}

pub struct PostingsCodec;

impl FormatCodec<PostingsList> for PostingsCodec {
    const KIND: BinaryFormatKind = BinaryFormatKind::Postings;
    const VERSION: u16 = 1;

    fn encode(value: &PostingsList) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(value.term.len() as u32).to_le_bytes());
        payload.extend_from_slice(&value.term);
        payload.extend_from_slice(&(value.record_ids.len() as u32).to_le_bytes());
        for rid in &value.record_ids {
            payload.extend_from_slice(&rid.to_le_bytes());
        }
        Ok(encode_envelope(Self::KIND, Self::VERSION, &payload))
    }

    fn decode(bytes: &[u8]) -> Result<PostingsList> {
        let payload = decode_envelope(bytes, Self::KIND, Self::VERSION)?;
        let mut offset = 0;
        let term_len = read_u32(payload, &mut offset, "postings term length")? as usize;
        let term = read_bytes(payload, &mut offset, term_len, "postings term")?;
        let count = read_u32(payload, &mut offset, "postings count")? as usize;
        let mut record_ids = Vec::with_capacity(count);
        for _ in 0..count {
            record_ids.push(read_u64(payload, &mut offset, "postings record id")?);
        }
        if offset != payload.len() {
            return Err(super::corrupt("postings trailing bytes"));
        }
        Ok(PostingsList { term, record_ids })
    }
}
