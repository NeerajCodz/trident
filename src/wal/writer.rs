use crate::config::WalSyncPolicy;
use crate::errors::{Result, TridentError};
use crate::io::BinaryWriter;
use crate::wal::record::{WAL_RECORD_HEADER_LEN, WalRecord};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const WAL_FILE_MAGIC: u32 = 0x5457_4146;
const WAL_FILE_VERSION: u32 = 1;
const WAL_FILE_HEADER_LEN: usize = 16;

#[derive(Debug)]
pub struct Wal {
    path: PathBuf,
    segment_id: u64,
    file: File,
    sync_policy: WalSyncPolicy,
    bytes_written: u64,
}

impl Wal {
    pub fn open(
        path: impl Into<PathBuf>,
        segment_id: u64,
        sync_policy: WalSyncPolicy,
    ) -> Result<Self> {
        let path = path.into();
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(false)
            .open(&path)?;
        let mut bytes_written = file.metadata()?.len();
        if bytes_written == 0 {
            let header = file_header(segment_id);
            file.write_all(&header)?;
            file.sync_data()?;
            bytes_written = header.len() as u64;
        }
        Ok(Self {
            path,
            segment_id,
            file,
            sync_policy,
            bytes_written,
        })
    }

    pub fn should_rotate(&self, next_record_len: usize, max_segment_bytes: usize) -> bool {
        self.bytes_written > WAL_FILE_HEADER_LEN as u64
            && self.bytes_written + next_record_len as u64 > max_segment_bytes as u64
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let encoded = record.encode();
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&encoded)?;
        self.bytes_written += encoded.len() as u64;
        if matches!(self.sync_policy, WalSyncPolicy::EveryBatch) {
            self.file.sync_data()?;
        }
        Ok(())
    }

    pub fn replay_dir(root: &Path) -> Result<Vec<WalRecord>> {
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(root)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("wal") {
                paths.push(path);
            }
        }
        paths.sort();
        let mut records = Vec::new();
        for path in paths {
            records.extend(Self::replay(&path)?);
        }
        records.sort_by_key(|record| record.sequence);
        Ok(records)
    }

    pub fn replay(path: &Path) -> Result<Vec<WalRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut offset = file_payload_offset(&bytes, path)?;
        let mut records = Vec::new();
        while offset + WAL_RECORD_HEADER_LEN <= bytes.len() {
            let len = u32::from_le_bytes([
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]) as usize;
            if offset + WAL_RECORD_HEADER_LEN + len > bytes.len() {
                break;
            }
            let record_bytes = &bytes[offset..offset + WAL_RECORD_HEADER_LEN + len];
            match WalRecord::decode(record_bytes, path) {
                Ok(record) => records.push(record),
                Err(_) => break,
            }
            offset += WAL_RECORD_HEADER_LEN + len;
        }
        Ok(records)
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn segment_id(&self) -> u64 {
        self.segment_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn file_header(segment_id: u64) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(WAL_FILE_MAGIC);
    writer.write_u32(WAL_FILE_VERSION);
    writer.write_u64(segment_id);
    writer.into_inner()
}

fn file_payload_offset(bytes: &[u8], path: &Path) -> Result<usize> {
    if bytes.len() < 4 {
        return Ok(0);
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != WAL_FILE_MAGIC {
        return Ok(0);
    }
    if bytes.len() < WAL_FILE_HEADER_LEN {
        return Err(TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: "torn WAL file header".to_string(),
        });
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != WAL_FILE_VERSION {
        return Err(TridentError::Corrupt {
            path: path.to_path_buf(),
            reason: format!("unsupported WAL file version {version}"),
        });
    }
    Ok(WAL_FILE_HEADER_LEN)
}
