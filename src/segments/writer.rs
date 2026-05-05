use crate::accel::Accelerator;
use crate::config::Compression;
use crate::errors::Result;
use crate::io::{BinaryWriter, file_digest};
use crate::manifest::SegmentMetadata;
use crate::segments::block::SegmentEntry;
use crate::types::StoredValue;
use std::fs;
use std::path::Path;

const SEGMENT_MAGIC: u32 = 0x5453_4547;

pub struct SegmentWriter;

impl SegmentWriter {
    pub fn write(
        path: &Path,
        id: u64,
        level: u32,
        compression: Compression,
        accelerator: &dyn Accelerator,
        mut entries: Vec<SegmentEntry>,
    ) -> Result<SegmentMetadata> {
        entries.sort_by(|left, right| {
            (&left.cf.0, left.key.as_ref()).cmp(&(&right.cf.0, right.key.as_ref()))
        });
        let mut payload = BinaryWriter::new();
        payload.write_u32(entries.len() as u32);
        for entry in &entries {
            payload.write_len_bytes(entry.cf.0.as_bytes());
            payload.write_len_bytes(&entry.key);
            payload.write_u64(entry.version.sequence);
            match &entry.version.value {
                StoredValue::Put(value) => {
                    payload.write_u8(1);
                    payload.write_len_bytes(value);
                }
                StoredValue::Delete => {
                    payload.write_u8(2);
                }
            }
        }
        let compressed = accelerator.encode_block(compression, &payload.into_inner())?;
        let mut out = BinaryWriter::new();
        out.write_u32(SEGMENT_MAGIC);
        out.write_u8(match compression {
            Compression::None => 0,
            Compression::Lz4 => 1,
            Compression::Zstd => 2,
        });
        out.write_u32(accelerator.crc32c(&compressed));
        out.write_len_bytes(&compressed);
        fs::write(path, out.into_inner())?;
        let digest = file_digest(path)?;
        let min_key = entries
            .first()
            .map(|entry| entry.key.to_vec())
            .unwrap_or_default();
        let max_key = entries
            .last()
            .map(|entry| entry.key.to_vec())
            .unwrap_or_default();
        Ok(SegmentMetadata {
            id,
            level,
            path: path.to_string_lossy().to_string(),
            min_key,
            max_key,
            entries: entries.len() as u64,
            file_digest: digest.to_hex().to_string(),
        })
    }
}
