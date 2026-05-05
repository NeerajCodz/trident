use crate::accel::Accelerator;
use crate::config::Compression;
use crate::errors::{Result, TridentError};
use crate::io::{BinaryReader, crc32c};
use crate::segments::block::SegmentEntry;
use crate::segments::format::{
    BlockIndexEntry, SEGMENT_FOOTER_MAGIC, SEGMENT_FOOTER_TRAILER_LEN, SEGMENT_HEADER_LEN,
    SEGMENT_MAGIC, SEGMENT_VERSION,
};
use crate::types::{ColumnFamily, StoredValue, ValuePointer, VersionedValue};
use bytes::Bytes;
use std::fs;
use std::path::{Path, PathBuf};

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
        let version = reader.read_u32()?;
        if version != SEGMENT_VERSION {
            return Err(TridentError::Corrupt {
                path: path.to_path_buf(),
                reason: format!("unsupported segment version {version}"),
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
        let _level = reader.read_u32()?;
        let _id = reader.read_u64()?;
        let index = read_footer(&bytes, path)?;
        let mut entries = Vec::new();
        for block in index {
            let start = block.offset as usize;
            let end = start + block.len as usize;
            if start < SEGMENT_HEADER_LEN || end > bytes.len() - SEGMENT_FOOTER_TRAILER_LEN {
                return Err(TridentError::Corrupt {
                    path: path.to_path_buf(),
                    reason: "segment block offset out of bounds".to_string(),
                });
            }
            entries.extend(read_block(
                &bytes[start..end],
                path,
                compression,
                accelerator,
            )?);
        }
        Ok(entries)
    }
}

fn read_footer(bytes: &[u8], path: &Path) -> Result<Vec<BlockIndexEntry>> {
    if bytes.len() < SEGMENT_HEADER_LEN + SEGMENT_FOOTER_TRAILER_LEN {
        return Err(TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: "segment too small".to_string(),
        });
    }
    let trailer = &bytes[bytes.len() - SEGMENT_FOOTER_TRAILER_LEN..];
    let footer_len = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]) as usize;
    let checksum = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    let magic = u32::from_le_bytes([trailer[8], trailer[9], trailer[10], trailer[11]]);
    if magic != SEGMENT_FOOTER_MAGIC {
        return Err(TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: "bad segment footer magic".to_string(),
        });
    }
    let footer_start = bytes
        .len()
        .checked_sub(SEGMENT_FOOTER_TRAILER_LEN + footer_len)
        .ok_or_else(|| TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: "segment footer out of bounds".to_string(),
        })?;
    let footer = &bytes[footer_start..footer_start + footer_len];
    if crc32c(footer) != checksum {
        return Err(TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: "segment footer checksum mismatch".to_string(),
        });
    }
    let mut reader = BinaryReader::new(footer, path);
    let count = reader.read_u32()?;
    let mut index = Vec::with_capacity(count as usize);
    for _ in 0..count {
        index.push(BlockIndexEntry {
            first_key: reader.read_len_bytes()?,
            last_key: reader.read_len_bytes()?,
            offset: reader.read_u64()?,
            len: reader.read_u64()?,
        });
    }
    Ok(index)
}

fn read_block(
    bytes: &[u8],
    path: &Path,
    compression: Compression,
    accelerator: &dyn Accelerator,
) -> Result<Vec<SegmentEntry>> {
    let mut reader = BinaryReader::new(bytes, path);
    let compressed_len = reader.read_u32()? as usize;
    let _uncompressed_len = reader.read_u32()?;
    let expected = reader.read_u32()?;
    if bytes.len() < 12 + compressed_len {
        return Err(TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: "segment block length out of bounds".to_string(),
        });
    }
    let compressed = &bytes[12..12 + compressed_len];
    if crc32c(compressed) != expected {
        return Err(TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: "segment block checksum mismatch".to_string(),
        });
    }
    let payload = accelerator.decode_block(compression, compressed)?;
    decode_block_entries(&payload, path)
}

fn decode_block_entries(payload: &[u8], path: &Path) -> Result<Vec<SegmentEntry>> {
    let mut payload_reader = BinaryReader::new(payload, path);
    let count = payload_reader.read_u32()?;
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let cf =
            ColumnFamily(String::from_utf8_lossy(&payload_reader.read_len_bytes()?).to_string());
        let key = Bytes::from(payload_reader.read_len_bytes()?);
        let sequence = payload_reader.read_u64()?;
        let value = match payload_reader.read_u8()? {
            1 => StoredValue::Put(payload_reader.read_len_bytes()?),
            2 => StoredValue::Delete,
            3 => {
                let path = String::from_utf8_lossy(&payload_reader.read_len_bytes()?).to_string();
                let offset = payload_reader.read_u64()?;
                let len = payload_reader.read_u64()?;
                let checksum = payload_reader.read_u32()?;
                StoredValue::BlobPointer(ValuePointer {
                    path,
                    offset,
                    len,
                    checksum,
                })
            }
            4 => {
                let expires_at_ms = payload_reader.read_u64()?;
                let path = String::from_utf8_lossy(&payload_reader.read_len_bytes()?).to_string();
                let offset = payload_reader.read_u64()?;
                let len = payload_reader.read_u64()?;
                let checksum = payload_reader.read_u32()?;
                StoredValue::PutWithExpiry {
                    value: crate::values::ValueLog::read_pointer(&ValuePointer {
                        path,
                        offset,
                        len,
                        checksum,
                    })?,
                    expires_at_ms,
                }
            }
            5 => {
                let expires_at_ms = payload_reader.read_u64()?;
                let value = payload_reader.read_len_bytes()?;
                StoredValue::PutWithExpiry {
                    value,
                    expires_at_ms,
                }
            }
            6 => StoredValue::Merge(payload_reader.read_len_bytes()?),
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
