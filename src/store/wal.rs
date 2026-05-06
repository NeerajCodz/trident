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
const DEFAULT_SEGMENT_BYTES: u64 = 16 * 1024 * 1024;

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
    dir: PathBuf,
    stem: String,
    active_segment_id: u64,
    active_segment_path: PathBuf,
    file: File,
    active_size: u64,
    max_segment_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageWalOptions {
    pub max_segment_bytes: u64,
}

impl Default for StorageWalOptions {
    fn default() -> Self {
        Self {
            max_segment_bytes: DEFAULT_SEGMENT_BYTES,
        }
    }
}

impl StorageWal {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with_options(path, StorageWalOptions::default())
    }

    pub fn open_with_options(path: impl Into<PathBuf>, options: StorageWalOptions) -> Result<Self> {
        let path = path.into();
        let dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&dir)?;
        let stem = wal_stem(&path);

        let mut segment_ids = list_segment_ids(&dir, &stem)?;
        segment_ids.sort_unstable();
        let active_segment_id = segment_ids.last().copied().unwrap_or(1);
        let active_segment_path = segment_path(&dir, &stem, active_segment_id);
        let (file, active_size) = open_segment_file(&active_segment_path)?;

        Ok(Self {
            path,
            dir,
            stem,
            active_segment_id,
            active_segment_path,
            file,
            active_size,
            max_segment_bytes: options.max_segment_bytes.max(1024),
        })
    }

    pub fn append(&mut self, entry: &StorageWalEntry) -> Result<()> {
        self.append_batch(std::slice::from_ref(entry))
    }

    pub fn append_batch(&mut self, entries: &[StorageWalEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let encoded: Vec<Vec<u8>> = entries
            .iter()
            .map(encode_record)
            .collect::<Result<Vec<_>>>()?;
        let batch_bytes: u64 = encoded.iter().map(|record| record.len() as u64).sum();

        if self.active_size > STORAGE_WAL_HEADER_LEN as u64
            && self.active_size + batch_bytes > self.max_segment_bytes
        {
            self.rotate_segment()?;
        }

        self.file.seek(SeekFrom::End(0))?;
        for record in &encoded {
            self.file.write_all(record)?;
        }
        self.file.sync_data()?;
        self.active_size += batch_bytes;
        Ok(())
    }

    pub fn replay(path: &Path) -> Result<Vec<StorageWalEntry>> {
        let dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let stem = wal_stem(path);
        let mut segment_ids = list_segment_ids(&dir, &stem)?;
        segment_ids.sort_unstable();
        if segment_ids.is_empty() {
            return replay_file(path);
        }

        let mut out = Vec::new();
        for segment_id in segment_ids {
            let segment = segment_path(&dir, &stem, segment_id);
            out.extend(replay_file(&segment)?);
        }
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn rotate_segment(&mut self) -> Result<()> {
        self.file.sync_data()?;
        self.active_segment_id = self.active_segment_id.saturating_add(1);
        self.active_segment_path = segment_path(&self.dir, &self.stem, self.active_segment_id);
        let (file, active_size) = open_segment_file(&self.active_segment_path)?;
        self.file = file;
        self.active_size = active_size;
        Ok(())
    }
}

fn wal_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("storage")
        .to_string()
}

fn segment_path(dir: &Path, stem: &str, segment_id: u64) -> PathBuf {
    dir.join(format!("{stem}.{segment_id:020}.swal"))
}

fn list_segment_ids(dir: &Path, stem: &str) -> Result<Vec<u64>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let prefix = format!("{stem}.");
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("swal") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        let middle = name
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.strip_suffix(".swal"));
        if let Some(raw_id) = middle
            && let Ok(segment_id) = raw_id.parse::<u64>()
        {
            ids.push(segment_id);
        }
    }
    Ok(ids)
}

fn open_segment_file(path: &Path) -> Result<(File, u64)> {
    let is_new = !path.exists();
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    if is_new {
        file.write_all(&STORAGE_WAL_MAGIC.to_le_bytes())?;
        file.write_all(&[STORAGE_WAL_VERSION])?;
        file.sync_data()?;
    }
    let size = file.metadata()?.len();
    Ok((file, size))
}

fn replay_file(path: &Path) -> Result<Vec<StorageWalEntry>> {
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
