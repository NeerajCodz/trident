use crate::accel::Accelerator;
use crate::config::Compression;
use crate::errors::Result;
use crate::io::{BinaryWriter, file_digest};
use crate::manifest::SegmentMetadata;
use crate::segments::block::SegmentEntry;
use crate::segments::bloom::{BloomFilter, bloom_key};
use crate::types::StoredValue;
use crate::values::ValueLog;
use std::fs;
use std::path::Path;

const SEGMENT_MAGIC: u32 = 0x5453_4547;

pub struct SegmentWriter;

pub struct SegmentWriteOptions<'a> {
    pub path: &'a Path,
    pub id: u64,
    pub level: u32,
    pub compression: Compression,
    pub accelerator: &'a dyn Accelerator,
    pub value_log: &'a mut ValueLog,
    pub large_value_threshold: usize,
}

impl SegmentWriter {
    pub fn write(
        options: SegmentWriteOptions<'_>,
        mut entries: Vec<SegmentEntry>,
    ) -> Result<SegmentMetadata> {
        entries.sort_by(|left, right| {
            (&left.cf.0, left.key.as_ref()).cmp(&(&right.cf.0, right.key.as_ref()))
        });
        let mut payload = BinaryWriter::new();
        let mut bloom_filter = BloomFilter::with_expected_items(entries.len());
        payload.write_u32(entries.len() as u32);
        for entry in &entries {
            bloom_filter.insert(&bloom_key(&entry.cf.0, &entry.key));
            payload.write_len_bytes(entry.cf.0.as_bytes());
            payload.write_len_bytes(&entry.key);
            payload.write_u64(entry.version.sequence);
            match &entry.version.value {
                StoredValue::Put(value) => {
                    if value.len() > options.large_value_threshold {
                        let pointer = options.value_log.append(value)?;
                        payload.write_u8(3);
                        payload.write_len_bytes(pointer.path.as_bytes());
                        payload.write_u64(pointer.offset);
                        payload.write_u64(pointer.len);
                        payload.write_u32(pointer.checksum);
                    } else {
                        payload.write_u8(1);
                        payload.write_len_bytes(value);
                    }
                }
                StoredValue::BlobPointer(pointer) => {
                    payload.write_u8(3);
                    payload.write_len_bytes(pointer.path.as_bytes());
                    payload.write_u64(pointer.offset);
                    payload.write_u64(pointer.len);
                    payload.write_u32(pointer.checksum);
                }
                StoredValue::Delete => {
                    payload.write_u8(2);
                }
            }
        }
        let compressed = options
            .accelerator
            .encode_block(options.compression, &payload.into_inner())?;
        let mut out = BinaryWriter::new();
        out.write_u32(SEGMENT_MAGIC);
        out.write_u8(match options.compression {
            Compression::None => 0,
            Compression::Lz4 => 1,
            Compression::Zstd => 2,
        });
        out.write_u32(options.accelerator.crc32c(&compressed));
        out.write_len_bytes(&compressed);
        fs::write(options.path, out.into_inner())?;
        let digest = file_digest(options.path)?;
        let min_key = entries
            .first()
            .map(|entry| entry.key.to_vec())
            .unwrap_or_default();
        let max_key = entries
            .last()
            .map(|entry| entry.key.to_vec())
            .unwrap_or_default();
        Ok(SegmentMetadata {
            id: options.id,
            level: options.level,
            path: options.path.to_string_lossy().to_string(),
            min_key,
            max_key,
            entries: entries.len() as u64,
            bloom_filter,
            file_digest: digest.to_hex().to_string(),
        })
    }
}
