use crate::accel::Accelerator;
use crate::config::Compression;
use crate::errors::{Result, TridentError};
use crate::io::{BinaryReader, crc32c};
use crate::segments::block::SegmentEntry;
use crate::types::{ColumnFamily, StoredValue, VersionedValue};
use bytes::Bytes;
use std::fs;
use std::path::{Path, PathBuf};

const SEGMENT_MAGIC: u32 = 0x5453_4547;

pub struct SegmentReader;

impl SegmentReader {
    pub fn read(path: &Path, accelerator: &dyn Accelerator) -> Result<Vec<SegmentEntry>> {
        let bytes = fs::read(path)?;
        let mut reader = BinaryReader::new(&bytes, path);
        let magic = reader.read_u32()?;
        if magic != SEGMENT_MAGIC {
            return Err(TridentError::Corrupt {
                path: path.to_path_buf(),
                reason: "bad segment magic".to_string(),
            });
        }
        let compression = match reader.read_u8()? {
            0 => Compression::None,
            1 => Compression::Lz4,
            2 => Compression::Zstd,
            tag => {
                return Err(TridentError::Corrupt {
                    path: path.to_path_buf(),
                    reason: format!("unknown segment compression tag {tag}"),
                });
            }
        };
        let expected = reader.read_u32()?;
        let compressed = reader.read_len_bytes()?;
        if crc32c(&compressed) != expected {
            return Err(TridentError::Corrupt {
                path: path.to_path_buf(),
                reason: "segment checksum mismatch".to_string(),
            });
        }
        let payload = accelerator.decode_block(compression, &compressed)?;
        let mut payload_reader = BinaryReader::new(&payload, path);
        let count = payload_reader.read_u32()?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let cf = ColumnFamily(
                String::from_utf8_lossy(&payload_reader.read_len_bytes()?).to_string(),
            );
            let key = Bytes::from(payload_reader.read_len_bytes()?);
            let sequence = payload_reader.read_u64()?;
            let value = match payload_reader.read_u8()? {
                1 => StoredValue::Put(payload_reader.read_len_bytes()?),
                2 => StoredValue::Delete,
                tag => {
                    return Err(TridentError::Corrupt {
                        path: PathBuf::from("segment-payload"),
                        reason: format!("unknown segment value tag {tag}"),
                    });
                }
            };
            entries.push(SegmentEntry {
                cf,
                key,
                version: VersionedValue { sequence, value },
            });
        }
        Ok(entries)
    }
}
