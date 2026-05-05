use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub last_sequence: u64,
    pub next_segment_id: u64,
    pub segments: Vec<SegmentMetadata>,
}

impl Manifest {
    pub fn fresh() -> Self {
        Self {
            format_version: 1,
            last_sequence: 0,
            next_segment_id: 1,
            segments: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SegmentMetadata {
    pub id: u64,
    pub level: u32,
    pub path: String,
    pub min_key: Vec<u8>,
    pub max_key: Vec<u8>,
    pub entries: u64,
    pub file_digest: String,
}
