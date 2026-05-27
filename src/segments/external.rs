use crate::datatype::SegmentFamily;
use crate::errors::{PraxisError, Result};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const BLOB_MAGIC: u32 = 0x5442_4c4f;
const BLOB_VERSION: u8 = 1;
const HEADER_LEN: u64 = 5;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlobLocation {
    pub family: Option<SegmentFamily>,
    pub file_id: u64,
    pub offset: u64,
    pub len: u32,
    pub checksum: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobStore {
    path: PathBuf,
    family: Option<SegmentFamily>,
    file_id: u64,
}

impl BlobStore {
    pub fn open_overflow(path: impl Into<PathBuf>) -> Result<Self> {
        Self::open(path, None, 1)
    }

    pub fn open_segment(
        path: impl Into<PathBuf>,
        family: SegmentFamily,
        file_id: u64,
    ) -> Result<Self> {
        Self::open(path, Some(family), file_id)
    }

    fn open(path: impl Into<PathBuf>, family: Option<SegmentFamily>, file_id: u64) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&path)?;
            file.write_all(&BLOB_MAGIC.to_le_bytes())?;
            file.write_all(&[BLOB_VERSION])?;
            file.sync_all()?;
        }
        Ok(Self {
            path,
            family,
            file_id,
        })
    }

    pub fn append(&self, bytes: &[u8]) -> Result<BlobLocation> {
        let mut file = OpenOptions::new()
            .append(true)
            .read(true)
            .open(&self.path)?;
        let offset = file.seek(SeekFrom::End(0))?;
        let checksum = crc32c(bytes);
        let mut writer = BinaryWriter::new();
        writer.write_u32(bytes.len() as u32);
        writer.write_u32(checksum);
        writer.write_bytes(bytes);
        file.write_all(&writer.into_inner())?;
        file.sync_data()?;
        Ok(BlobLocation {
            family: self.family,
            file_id: self.file_id,
            offset,
            len: bytes.len() as u32,
            checksum,
        })
    }

    pub fn read(&self, location: &BlobLocation) -> Result<Vec<u8>> {
        if location.family != self.family || location.file_id != self.file_id {
            return Err(PraxisError::InvalidConfig(
                "blob location does not belong to this store".to_string(),
            ));
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.len() < HEADER_LEN as usize {
            return corrupt(&self.path, "truncated blob store header");
        }
        if u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != BLOB_MAGIC {
            return corrupt(&self.path, "bad blob store magic");
        }
        if bytes[4] != BLOB_VERSION {
            return corrupt(
                &self.path,
                &format!("unsupported blob version {}", bytes[4]),
            );
        }
        let start = location.offset as usize;
        if start < HEADER_LEN as usize || start.saturating_add(8) > bytes.len() {
            return corrupt(&self.path, "blob location outside file bounds");
        }
        let mut reader = BinaryReader::new(&bytes[start..], self.path.clone());
        let len = reader.read_u32()?;
        let expected = reader.read_u32()?;
        if len != location.len || (location.checksum != 0 && expected != location.checksum) {
            return corrupt(&self.path, "blob location metadata mismatch");
        }
        let mut data = Vec::with_capacity(len as usize);
        for _ in 0..len {
            data.push(reader.read_u8()?);
        }
        let actual = crc32c(&data);
        if actual != expected {
            return corrupt(
                &self.path,
                &format!("blob checksum mismatch: expected {expected:#010x}, got {actual:#010x}"),
            );
        }
        Ok(data)
    }
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(PraxisError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
