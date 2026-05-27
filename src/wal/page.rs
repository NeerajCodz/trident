use crate::config::WalSyncPolicy;
use crate::errors::{PraxisError, Result};
use crate::identity::{Aid, Cid, Did, Eid, FieldId, Rid};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const PAGE_WAL_FILE_MAGIC: u32 = 0x5450_5746;
const PAGE_WAL_FILE_VERSION: u32 = 1;
const PAGE_WAL_FILE_HEADER_LEN: usize = 16;
const PAGE_WAL_RECORD_MAGIC: u32 = 0x5450_5752;
const PAGE_WAL_RECORD_VERSION: u8 = 1;
const PAGE_WAL_RECORD_HEADER_LEN: usize = 13;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageWalMutation {
    Put {
        cid: Cid,
        eid: Eid,
        rid: Rid,
        fields: Vec<(FieldId, Vec<u8>)>,
    },
    Delete {
        cid: Cid,
        eid: Eid,
        rid: Rid,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageWalRecord {
    pub sequence: u64,
    pub mutation: PageWalMutation,
}

#[derive(Debug)]
pub struct PageWal {
    path: PathBuf,
    file: File,
    sync_policy: WalSyncPolicy,
    next_sequence: u64,
    bytes_written: u64,
}

impl PageWalRecord {
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = BinaryWriter::new();
        payload.write_u64(self.sequence);
        match &self.mutation {
            PageWalMutation::Put {
                cid,
                eid,
                rid,
                fields,
            } => {
                payload.write_u8(1);
                payload.write_u32(cid.0);
                payload.write_u32(eid.0);
                payload.write_u64(rid.0);
                payload.write_u32(fields.len() as u32);
                for (field, bytes) in fields {
                    write_field(&mut payload, *field);
                    payload.write_len_bytes(bytes);
                }
            }
            PageWalMutation::Delete { cid, eid, rid } => {
                payload.write_u8(2);
                payload.write_u32(cid.0);
                payload.write_u32(eid.0);
                payload.write_u64(rid.0);
            }
        }
        let payload = payload.into_inner();
        let mut out = BinaryWriter::new();
        out.write_u32(PAGE_WAL_RECORD_MAGIC);
        out.write_u8(PAGE_WAL_RECORD_VERSION);
        out.write_u32(payload.len() as u32);
        out.write_u32(crc32c(&payload));
        out.write_bytes(&payload);
        out.into_inner()
    }

    pub fn decode(bytes: &[u8], source: impl Into<PathBuf>) -> Result<Self> {
        let source = source.into();
        if bytes.len() < PAGE_WAL_RECORD_HEADER_LEN {
            return corrupt(&source, "truncated page WAL record header");
        }
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != PAGE_WAL_RECORD_MAGIC {
            return corrupt(&source, "bad page WAL record magic");
        }
        if bytes[4] != PAGE_WAL_RECORD_VERSION {
            return corrupt(
                &source,
                &format!("unsupported page WAL record version {}", bytes[4]),
            );
        }
        let len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        let expected = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
        if bytes.len() < PAGE_WAL_RECORD_HEADER_LEN + len {
            return corrupt(&source, "torn page WAL record");
        }
        let payload = &bytes[PAGE_WAL_RECORD_HEADER_LEN..PAGE_WAL_RECORD_HEADER_LEN + len];
        let actual = crc32c(payload);
        if actual != expected {
            return corrupt(
                &source,
                &format!(
                    "page WAL checksum mismatch: expected {expected:#010x}, got {actual:#010x}"
                ),
            );
        }
        let mut reader = BinaryReader::new(payload, source.clone());
        let sequence = reader.read_u64()?;
        let tag = reader.read_u8()?;
        let mutation = match tag {
            1 => {
                let cid = Cid(reader.read_u32()?);
                let eid = Eid(reader.read_u32()?);
                let rid = Rid(reader.read_u64()?);
                let count = reader.read_u32()? as usize;
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count {
                    let field = read_field(&mut reader, &source)?;
                    let bytes = reader.read_len_bytes()?;
                    fields.push((field, bytes));
                }
                PageWalMutation::Put {
                    cid,
                    eid,
                    rid,
                    fields,
                }
            }
            2 => PageWalMutation::Delete {
                cid: Cid(reader.read_u32()?),
                eid: Eid(reader.read_u32()?),
                rid: Rid(reader.read_u64()?),
            },
            _ => return corrupt(&source, &format!("unknown page WAL mutation tag {tag}")),
        };
        Ok(Self { sequence, mutation })
    }
}

impl PageWal {
    pub fn open(path: impl Into<PathBuf>, sync_policy: WalSyncPolicy) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        let mut bytes_written = file.metadata()?.len();
        if bytes_written == 0 {
            let header = file_header();
            file.write_all(&header)?;
            file.sync_data()?;
            bytes_written = header.len() as u64;
        }
        let next_sequence = Self::replay(&path)?
            .iter()
            .map(|record| record.sequence)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(Self {
            path,
            file,
            sync_policy,
            next_sequence,
            bytes_written,
        })
    }

    pub fn append_put(
        &mut self,
        cid: Cid,
        eid: Eid,
        rid: Rid,
        fields: &[(FieldId, &[u8])],
    ) -> Result<PageWalRecord> {
        let record = PageWalRecord {
            sequence: self.claim_sequence(),
            mutation: PageWalMutation::Put {
                cid,
                eid,
                rid,
                fields: fields
                    .iter()
                    .map(|(field, bytes)| (*field, (*bytes).to_vec()))
                    .collect(),
            },
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn append_delete(&mut self, cid: Cid, eid: Eid, rid: Rid) -> Result<PageWalRecord> {
        let record = PageWalRecord {
            sequence: self.claim_sequence(),
            mutation: PageWalMutation::Delete { cid, eid, rid },
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn append(&mut self, record: &PageWalRecord) -> Result<()> {
        let encoded = record.encode();
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&encoded)?;
        self.bytes_written = self.bytes_written.saturating_add(encoded.len() as u64);
        if matches!(self.sync_policy, WalSyncPolicy::EveryBatch) {
            self.file.sync_data()?;
        }
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.sync_data()?;
        Ok(())
    }

    pub fn replay(path: &Path) -> Result<Vec<PageWalRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut offset = file_payload_offset(&bytes, path)?;
        let mut records = Vec::new();
        while offset + PAGE_WAL_RECORD_HEADER_LEN <= bytes.len() {
            let len = u32::from_le_bytes([
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
                bytes[offset + 8],
            ]) as usize;
            if offset + PAGE_WAL_RECORD_HEADER_LEN + len > bytes.len() {
                break;
            }
            let record_bytes = &bytes[offset..offset + PAGE_WAL_RECORD_HEADER_LEN + len];
            match PageWalRecord::decode(record_bytes, path) {
                Ok(record) => records.push(record),
                Err(_) => break,
            }
            offset += PAGE_WAL_RECORD_HEADER_LEN + len;
        }
        Ok(records)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    fn claim_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }
}

fn write_field(writer: &mut BinaryWriter, field: FieldId) {
    match field {
        FieldId::Fixed(aid) => {
            writer.write_u8(1);
            writer.write_u32(aid.0 as u32);
        }
        FieldId::Dynamic(did) => {
            writer.write_u8(2);
            writer.write_u32(did.0 as u32);
        }
    }
}

fn read_field(reader: &mut BinaryReader<'_>, source: &Path) -> Result<FieldId> {
    let tag = reader.read_u8()?;
    let raw = reader.read_u32()? as u16;
    match tag {
        1 => Ok(FieldId::Fixed(Aid(raw))),
        2 => Ok(FieldId::Dynamic(Did(raw))),
        _ => corrupt(source, &format!("unknown page WAL field tag {tag}")),
    }
}

fn file_header() -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(PAGE_WAL_FILE_MAGIC);
    writer.write_u32(PAGE_WAL_FILE_VERSION);
    writer.write_u64(0);
    writer.into_inner()
}

fn file_payload_offset(bytes: &[u8], path: &Path) -> Result<usize> {
    if bytes.len() < 4 {
        return Ok(0);
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != PAGE_WAL_FILE_MAGIC {
        return Ok(0);
    }
    if bytes.len() < PAGE_WAL_FILE_HEADER_LEN {
        return corrupt(path, "torn page WAL file header");
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != PAGE_WAL_FILE_VERSION {
        return corrupt(
            path,
            &format!("unsupported page WAL file version {version}"),
        );
    }
    Ok(PAGE_WAL_FILE_HEADER_LEN)
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(PraxisError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
