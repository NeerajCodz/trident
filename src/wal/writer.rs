use crate::config::WalSyncPolicy;
use crate::errors::Result;
use crate::wal::record::WalRecord;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Wal {
    path: PathBuf,
    file: File,
    sync_policy: WalSyncPolicy,
}

impl Wal {
    pub fn open(path: impl Into<PathBuf>, sync_policy: WalSyncPolicy) -> Result<Self> {
        let path = path.into();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(false)
            .open(&path)?;
        Ok(Self {
            path,
            file,
            sync_policy,
        })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let encoded = record.encode();
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&encoded)?;
        if matches!(self.sync_policy, WalSyncPolicy::EveryBatch) {
            self.file.sync_data()?;
        }
        Ok(())
    }

    pub fn replay(path: &Path) -> Result<Vec<WalRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut offset = 0usize;
        let mut records = Vec::new();
        while offset + 12 <= bytes.len() {
            let len = u32::from_le_bytes([
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]) as usize;
            if offset + 12 + len > bytes.len() {
                break;
            }
            let record_bytes = &bytes[offset..offset + 12 + len];
            match WalRecord::decode(record_bytes, path) {
                Ok(record) => records.push(record),
                Err(_) => break,
            }
            offset += 12 + len;
        }
        Ok(records)
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
