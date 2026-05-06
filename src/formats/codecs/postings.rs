use super::{BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope};
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
        let term_len = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
        let term = payload[4..4 + term_len].to_vec();
        let mut offset = 4 + term_len;
        let count = u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut record_ids = Vec::with_capacity(count);
        for _ in 0..count {
            record_ids.push(u64::from_le_bytes(
                payload[offset..offset + 8].try_into().unwrap(),
            ));
            offset += 8;
        }
        Ok(PostingsList { term, record_ids })
    }
}
