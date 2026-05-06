use super::RecordId;
use crate::errors::{Result, TridentError};
use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const STORAGE_WAL_MAGIC: u32 = 0x5453_574c;
const STORAGE_WAL_VERSION: u8 = 1;
const STORAGE_WAL_HEADER_LEN: usize = 5;
const STORAGE_WAL_RECORD_HEADER_LEN: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StorageWalOperation {
    Put,
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StorageWalEntry {
    pub sequence: u64,
    pub index_type: String,
    pub key: Vec<u8>,
    pub rid: Option<RecordId>,
    pub operation: StorageWalOperation,
}

pub struct StorageWal {
    path: PathBuf,
    file: File,
}

impl StorageWal {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let is_new = !path.exists();
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        if is_new {
            file.write_all(&STORAGE_WAL_MAGIC.to_le_bytes())?;
            file.write_all(&[STORAGE_WAL_VERSION])?;
            file.sync_data()?;
        }
        Ok(Self { path, file })
    }

    pub fn append(&mut self, entry: &StorageWalEntry) -> Result<()> {
        self.append_batch(std::slice::from_ref(entry))
    }

    pub fn append_batch(&mut self, entries: &[StorageWalEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        self.file.seek(SeekFrom::End(0))?;
        for entry in entries {
            let encoded = encode_record(entry)?;
            self.file.write_all(&encoded)?;
        }
        self.file.sync_data()?;
        Ok(())
    }

    pub fn replay(path: &Path) -> Result<Vec<StorageWalEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes.len() < STORAGE_WAL_HEADER_LEN {
            return Ok(Vec::new());
        }
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != STORAGE_WAL_MAGIC {
            return Err(TridentError::Corrupt {
                path: path.to_path_buf(),
                reason: "bad storage WAL magic".to_string(),
            });
        }
        if bytes[4] != STORAGE_WAL_VERSION {
            return Err(TridentError::Corrupt {
                path: path.to_path_buf(),
                reason: format!("unsupported storage WAL version {}", bytes[4]),
            });
        }

        let mut out = Vec::new();
        let mut cursor = STORAGE_WAL_HEADER_LEN;
        while cursor + STORAGE_WAL_RECORD_HEADER_LEN <= bytes.len() {
            let len = u32::from_le_bytes([
                bytes[cursor],
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
            ]) as usize;
            let expected = u32::from_le_bytes([
                bytes[cursor + 4],
                bytes[cursor + 5],
                bytes[cursor + 6],
                bytes[cursor + 7],
            ]);
            let start = cursor + STORAGE_WAL_RECORD_HEADER_LEN;
            let end = start + len;
            if end > bytes.len() {
                break;
            }
            let payload = &bytes[start..end];
            if crc32(payload) != expected {
                break;
            }
            let entry: StorageWalEntry = serde_json::from_slice(payload)?;
            out.push(entry);
            cursor = end;
        }
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut hasher = Crc32Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

fn encode_record(entry: &StorageWalEntry) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(entry)?;
    let checksum = crc32(&payload);
    let mut out = Vec::with_capacity(STORAGE_WAL_RECORD_HEADER_LEN + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&checksum.to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}
