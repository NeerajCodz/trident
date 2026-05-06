use super::{BinaryFormatKind, FormatCodec, decode_envelope, encode_envelope};
use crate::errors::Result;

#[derive(Clone, Debug, PartialEq)]
pub struct VectorGraphNode {
    pub record_id: u64,
    pub vector: Vec<f32>,
    pub neighbors: Vec<u64>,
}

pub struct VectorGraphCodec;

impl FormatCodec<VectorGraphNode> for VectorGraphCodec {
    const KIND: BinaryFormatKind = BinaryFormatKind::VectorGraph;
    const VERSION: u16 = 1;

    fn encode(value: &VectorGraphNode) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&value.record_id.to_le_bytes());
        payload.extend_from_slice(&(value.vector.len() as u32).to_le_bytes());
        for item in &value.vector {
            payload.extend_from_slice(&item.to_le_bytes());
        }
        payload.extend_from_slice(&(value.neighbors.len() as u32).to_le_bytes());
        for item in &value.neighbors {
            payload.extend_from_slice(&item.to_le_bytes());
        }
        Ok(encode_envelope(Self::KIND, Self::VERSION, &payload))
    }

    fn decode(bytes: &[u8]) -> Result<VectorGraphNode> {
        let payload = decode_envelope(bytes, Self::KIND, Self::VERSION)?;
        let record_id = u64::from_le_bytes(payload[0..8].try_into().unwrap());
        let mut offset = 8;
        let vector_len =
            u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut vector = Vec::with_capacity(vector_len);
        for _ in 0..vector_len {
            vector.push(f32::from_le_bytes(
                payload[offset..offset + 4].try_into().unwrap(),
            ));
            offset += 4;
        }
        let neighbor_len =
            u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut neighbors = Vec::with_capacity(neighbor_len);
        for _ in 0..neighbor_len {
            neighbors.push(u64::from_le_bytes(
                payload[offset..offset + 8].try_into().unwrap(),
            ));
            offset += 8;
        }
        Ok(VectorGraphNode {
            record_id,
            vector,
            neighbors,
        })
    }
}
