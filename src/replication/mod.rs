use crate::errors::{Result, TridentError};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const REPLICATION_LOG_MAGIC: u32 = 0x5452_504c;
const REPLICATION_LOG_VERSION: u8 = 1;
const HEADER_LEN: usize = 5;
const RECORD_HEADER_LEN: usize = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct LogPosition {
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplicationMode {
    Sync,
    Async,
    Quorum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplicationRecordKind {
    Put,
    Delete,
    Checkpoint,
    SnapshotChunk,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplicationRecord {
    pub position: LogPosition,
    pub kind: ReplicationRecordKind,
    pub rid: Option<RecordId>,
    pub payload: Vec<u8>,
}

impl ReplicationRecord {
    pub fn put(sequence: u64, rid: RecordId, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            position: LogPosition { sequence },
            kind: ReplicationRecordKind::Put,
            rid: Some(rid),
            payload: payload.into(),
        }
    }

    pub fn delete(sequence: u64, rid: RecordId) -> Self {
        Self {
            position: LogPosition { sequence },
            kind: ReplicationRecordKind::Delete,
            rid: Some(rid),
            payload: Vec::new(),
        }
    }
}

pub trait ReplicationLog {
    fn append(&mut self, record: &ReplicationRecord) -> Result<()>;
    fn replay_from(&self, position: LogPosition) -> Result<Vec<ReplicationRecord>>;
}

pub struct FileReplicationLog {
    path: PathBuf,
}

impl FileReplicationLog {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            let mut bytes = Vec::with_capacity(HEADER_LEN);
            bytes.extend_from_slice(&REPLICATION_LOG_MAGIC.to_le_bytes());
            bytes.push(REPLICATION_LOG_VERSION);
            std::fs::write(&path, bytes)?;
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ReplicationLog for FileReplicationLog {
    fn append(&mut self, record: &ReplicationRecord) -> Result<()> {
        let mut file = std::fs::OpenOptions::new().append(true).open(&self.path)?;
        let payload = encode_record(record);
        use std::io::Write;
        file.write_all(&payload)?;
        file.sync_data()?;
        Ok(())
    }

    fn replay_from(&self, position: LogPosition) -> Result<Vec<ReplicationRecord>> {
        let bytes = std::fs::read(&self.path)?;
        Ok(decode_log(&bytes, &self.path)?
            .into_iter()
            .filter(|record| record.position >= position)
            .collect())
    }
}

fn encode_record(record: &ReplicationRecord) -> Vec<u8> {
    let mut payload = BinaryWriter::new();
    payload.write_u64(record.position.sequence);
    payload.write_u8(match record.kind {
        ReplicationRecordKind::Put => 1,
        ReplicationRecordKind::Delete => 2,
        ReplicationRecordKind::Checkpoint => 3,
        ReplicationRecordKind::SnapshotChunk => 4,
    });
    match record.rid {
        Some(rid) => {
            payload.write_u8(1);
            payload.write_u64(rid.0);
        }
        None => {
            payload.write_u8(0);
            payload.write_u64(0);
        }
    }
    payload.write_len_bytes(&record.payload);
    let payload = payload.into_inner();

    let mut out = BinaryWriter::new();
    out.write_u32(payload.len() as u32);
    out.write_u32(crc32c(&payload));
    out.write_bytes(&payload);
    out.into_inner()
}

fn decode_log(bytes: &[u8], path: &Path) -> Result<Vec<ReplicationRecord>> {
    if bytes.len() < HEADER_LEN {
        return corrupt(path, "truncated replication log header");
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != REPLICATION_LOG_MAGIC {
        return corrupt(path, "bad replication log magic");
    }
    if bytes[4] != REPLICATION_LOG_VERSION {
        return corrupt(
            path,
            &format!("unsupported replication log version {}", bytes[4]),
        );
    }

    let mut offset = HEADER_LEN;
    let mut out = Vec::new();
    while offset < bytes.len() {
        if offset + RECORD_HEADER_LEN > bytes.len() {
            break;
        }
        let len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let expected_crc = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        offset += RECORD_HEADER_LEN;
        if offset + len > bytes.len() {
            break;
        }
        let payload = &bytes[offset..offset + len];
        let actual_crc = crc32c(payload);
        if actual_crc != expected_crc {
            return corrupt(
                path,
                &format!(
                    "replication log checksum mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
                ),
            );
        }
        out.push(decode_record(payload, path)?);
        offset += len;
    }
    Ok(out)
}

fn decode_record(payload: &[u8], path: &Path) -> Result<ReplicationRecord> {
    let mut reader = BinaryReader::new(payload, path.to_path_buf());
    let sequence = reader.read_u64()?;
    let kind = match reader.read_u8()? {
        1 => ReplicationRecordKind::Put,
        2 => ReplicationRecordKind::Delete,
        3 => ReplicationRecordKind::Checkpoint,
        4 => ReplicationRecordKind::SnapshotChunk,
        tag => return corrupt(path, &format!("invalid replication record kind {tag}")),
    };
    let has_rid = reader.read_u8()?;
    let raw_rid = reader.read_u64()?;
    let rid = match has_rid {
        0 => None,
        1 => Some(RecordId(raw_rid)),
        tag => return corrupt(path, &format!("invalid replication rid tag {tag}")),
    };
    Ok(ReplicationRecord {
        position: LogPosition { sequence },
        kind,
        rid,
        payload: reader.read_len_bytes()?,
    })
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(TridentError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
