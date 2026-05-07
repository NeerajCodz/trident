use crate::errors::{Result, TridentError};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::store::RecordId;
use std::path::{Path, PathBuf};

const BTREE_PAGE_MAGIC: u32 = 0x5450_4742;
const BTREE_PAGE_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BTreePageId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BTreePageKind {
    Leaf,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BTreePageEntry {
    pub key: Vec<u8>,
    pub sequence: u64,
    pub rid: RecordId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BTreePage {
    pub page_id: BTreePageId,
    pub kind: BTreePageKind,
    pub page_lsn: u64,
    pub right_sibling: Option<BTreePageId>,
    entries: Vec<BTreePageEntry>,
}

impl BTreePage {
    pub fn leaf(page_id: BTreePageId, page_lsn: u64) -> Self {
        Self {
            page_id,
            kind: BTreePageKind::Leaf,
            page_lsn,
            right_sibling: None,
            entries: Vec::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<Vec<u8>>, sequence: u64, rid: RecordId) {
        self.entries.push(BTreePageEntry {
            key: key.into(),
            sequence,
            rid,
        });
        self.entries.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then(right.sequence.cmp(&left.sequence))
        });
    }

    pub fn entries(&self) -> &[BTreePageEntry] {
        &self.entries
    }

    pub fn find_at(&self, key: &[u8], sequence: u64) -> Option<RecordId> {
        self.entries
            .iter()
            .find(|entry| entry.key == key && entry.sequence <= sequence)
            .map(|entry| entry.rid)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = BinaryWriter::new();
        payload.write_u64(self.page_id.0);
        payload.write_u8(match self.kind {
            BTreePageKind::Leaf => 1,
            BTreePageKind::Internal => 2,
        });
        payload.write_u64(self.page_lsn);
        match self.right_sibling {
            Some(page_id) => {
                payload.write_u8(1);
                payload.write_u64(page_id.0);
            }
            None => {
                payload.write_u8(0);
                payload.write_u64(0);
            }
        }
        payload.write_u32(self.entries.len() as u32);
        for entry in &self.entries {
            payload.write_len_bytes(&entry.key);
            payload.write_u64(entry.sequence);
            payload.write_u64(entry.rid.0);
        }
        let payload = payload.into_inner();

        let mut out = BinaryWriter::new();
        out.write_u32(BTREE_PAGE_MAGIC);
        out.write_u8(BTREE_PAGE_VERSION);
        out.write_u32(payload.len() as u32);
        out.write_u32(crc32c(&payload));
        out.write_bytes(&payload);
        out.into_inner()
    }

    pub fn from_bytes(bytes: &[u8], source: impl Into<PathBuf>) -> Result<Self> {
        let source = source.into();
        if bytes.len() < 13 {
            return corrupt(&source, "truncated B-tree page header");
        }
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != BTREE_PAGE_MAGIC {
            return corrupt(&source, "bad B-tree page magic");
        }
        if bytes[4] != BTREE_PAGE_VERSION {
            return corrupt(
                &source,
                &format!("unsupported B-tree page version {}", bytes[4]),
            );
        }
        let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        let expected_crc = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
        if bytes.len() < 13 + payload_len {
            return corrupt(&source, "truncated B-tree page payload");
        }
        let payload = &bytes[13..13 + payload_len];
        let actual_crc = crc32c(payload);
        if actual_crc != expected_crc {
            return corrupt(
                &source,
                &format!(
                    "B-tree page checksum mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
                ),
            );
        }

        let mut reader = BinaryReader::new(payload, source.clone());
        let page_id = BTreePageId(reader.read_u64()?);
        let kind = match reader.read_u8()? {
            1 => BTreePageKind::Leaf,
            2 => BTreePageKind::Internal,
            tag => return corrupt(&source, &format!("invalid B-tree page kind {tag}")),
        };
        let page_lsn = reader.read_u64()?;
        let right_sibling = match reader.read_u8()? {
            0 => {
                let _ = reader.read_u64()?;
                None
            }
            1 => Some(BTreePageId(reader.read_u64()?)),
            tag => return corrupt(&source, &format!("invalid B-tree sibling tag {tag}")),
        };
        let count = reader.read_u32()? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(BTreePageEntry {
                key: reader.read_len_bytes()?,
                sequence: reader.read_u64()?,
                rid: RecordId(reader.read_u64()?),
            });
        }
        Ok(Self {
            page_id,
            kind,
            page_lsn,
            right_sibling,
            entries,
        })
    }
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(TridentError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
