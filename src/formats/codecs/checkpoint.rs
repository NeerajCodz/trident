use super::{decode_envelope, encode_envelope, read_u64, BinaryFormatKind, FormatCodec};
use crate::errors::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointBlock {
    pub sequence: u64,
    pub manifest_epoch: u64,
}

pub struct CheckpointCodec;

impl FormatCodec<CheckpointBlock> for CheckpointCodec {
    const KIND: BinaryFormatKind = BinaryFormatKind::Checkpoint;
    const VERSION: u16 = 1;

    fn encode(value: &CheckpointBlock) -> Result<Vec<u8>> {
        let mut payload = Vec::with_capacity(16);
        payload.extend_from_slice(&value.sequence.to_le_bytes());
        payload.extend_from_slice(&value.manifest_epoch.to_le_bytes());
        Ok(encode_envelope(Self::KIND, Self::VERSION, &payload))
    }

    fn decode(bytes: &[u8]) -> Result<CheckpointBlock> {
        let payload = decode_envelope(bytes, Self::KIND, Self::VERSION)?;
        let mut offset = 0;
        let sequence = read_u64(payload, &mut offset, "checkpoint sequence")?;
        let manifest_epoch = read_u64(payload, &mut offset, "checkpoint manifest epoch")?;
        if offset != payload.len() {
            return Err(super::corrupt("checkpoint trailing bytes"));
        }
        Ok(CheckpointBlock {
            sequence,
            manifest_epoch,
        })
    }
}
