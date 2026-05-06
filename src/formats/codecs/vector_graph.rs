use super::{
    decode_envelope, encode_envelope, read_f32, read_u32, read_u64, BinaryFormatKind, FormatCodec,
};
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
        let mut offset = 0;
        let record_id = read_u64(payload, &mut offset, "vector graph record id")?;
        let vector_len = read_u32(payload, &mut offset, "vector length")? as usize;
        let mut vector = Vec::with_capacity(vector_len);
        for _ in 0..vector_len {
            vector.push(read_f32(payload, &mut offset, "vector component")?);
        }
        let neighbor_len = read_u32(payload, &mut offset, "neighbor count")? as usize;
        let mut neighbors = Vec::with_capacity(neighbor_len);
        for _ in 0..neighbor_len {
            neighbors.push(read_u64(payload, &mut offset, "neighbor record id")?);
        }
        if offset != payload.len() {
            return Err(super::corrupt("vector graph trailing bytes"));
        }
        Ok(VectorGraphNode {
            record_id,
            vector,
            neighbors,
        })
    }
}
