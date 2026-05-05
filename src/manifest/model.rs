use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub last_sequence: u64,
    pub next_segment_id: u64,
    pub latest_checkpoint: Option<CheckpointMetadata>,
    pub column_families: Vec<ColumnFamilyDescriptor>,
    pub segments: Vec<SegmentMetadata>,
}

impl Manifest {
    pub fn fresh() -> Self {
        Self {
            format_version: 1,
            last_sequence: 0,
            next_segment_id: 1,
            latest_checkpoint: None,
            column_families: vec![ColumnFamilyDescriptor::default()],
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
    pub bloom_filter: crate::segments::BloomFilter,
    pub file_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    pub id: u64,
    pub path: String,
    pub sequence: u64,
    pub segment_count: u64,
    pub file_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ColumnFamilyDescriptor {
    pub name: String,
    pub write_buffer_size_bytes: usize,
    pub large_value_threshold_bytes: usize,
    pub bloom_enabled: bool,
}

impl Default for ColumnFamilyDescriptor {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            write_buffer_size_bytes: 64 * 1024 * 1024,
            large_value_threshold_bytes: 64 * 1024,
            bloom_enabled: true,
        }
    }
}
