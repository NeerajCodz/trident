pub mod bitmap;
pub mod checkpoint;
pub mod columnar;
pub mod filter;
pub mod postings;
pub mod row;
pub mod vector_graph;

use crate::errors::{Result, TridentError};
use std::path::PathBuf;

pub const FORMAT_MAGIC: &[u8; 4] = b"TFMT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryFormatKind {
    RowRecord = 1,
    ColumnarBlock = 2,
    Postings = 3,
    Bitmap = 4,
    VectorGraph = 5,
    Filter = 6,
    Checkpoint = 7,
}

pub trait FormatCodec<T> {
    const KIND: BinaryFormatKind;
    const VERSION: u16;

    fn encode(value: &T) -> Result<Vec<u8>>;
    fn decode(bytes: &[u8]) -> Result<T>;
}

pub fn encode_envelope(kind: BinaryFormatKind, version: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + payload.len());
    out.extend_from_slice(FORMAT_MAGIC);
    out.extend_from_slice(&(kind as u16).to_le_bytes());
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn decode_envelope(bytes: &[u8], kind: BinaryFormatKind, version: u16) -> Result<&[u8]> {
    if bytes.len() < 12 || &bytes[..4] != FORMAT_MAGIC {
        return Err(corrupt("invalid format envelope"));
    }
    let actual_kind = u16::from_le_bytes([bytes[4], bytes[5]]);
    let actual_version = u16::from_le_bytes([bytes[6], bytes[7]]);
    let payload_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if actual_kind != kind as u16 || actual_version != version {
        return Err(corrupt("format kind or version mismatch"));
    }
    if bytes.len() != 12 + payload_len {
        return Err(corrupt("format payload length mismatch"));
    }
    Ok(&bytes[12..])
}

pub fn corrupt(reason: impl Into<String>) -> TridentError {
    TridentError::Corrupt {
        path: PathBuf::from("<memory>"),
        reason: reason.into(),
    }
}
