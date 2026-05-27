use crate::errors::{PraxisError, Result};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::store::RecordId;
use std::path::{Path, PathBuf};

const BTREE_PAGE_MAGIC: u32 = 0x5450_4742;
const BTREE_PAGE_VERSION: u8 = 2;
const DEFAULT_PAGE_SIZE: u32 = 16 * 1024;

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
    pub ghost: bool,
    pub overflow_page: Option<BTreePageId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BTreePageSlot {
    pub offset: u32,
    pub len: u32,
    pub ghost: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BTreeSlottedLayout {
    pub page_size: u32,
    pub header_len: u32,
    pub free_space_start: u32,
    pub free_space_end: u32,
    pub slot_directory: Vec<BTreePageSlot>,
    pub overflow_page_count: u32,
    pub compressed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BTreePage {
    pub page_id: BTreePageId,
    pub kind: BTreePageKind,
    pub page_lsn: u64,
    pub left_sibling: Option<BTreePageId>,
    pub right_sibling: Option<BTreePageId>,
    pub low_fence: Option<Vec<u8>>,
    pub high_fence: Option<Vec<u8>>,
    pub compressed: bool,
    entries: Vec<BTreePageEntry>,
}

impl BTreePage {
    pub fn leaf(page_id: BTreePageId, page_lsn: u64) -> Self {
        Self {
            page_id,
            kind: BTreePageKind::Leaf,
            page_lsn,
            left_sibling: None,
            right_sibling: None,
            low_fence: None,
            high_fence: None,
            compressed: false,
            entries: Vec::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<Vec<u8>>, sequence: u64, rid: RecordId) {
        self.entries.push(BTreePageEntry {
            key: key.into(),
            sequence,
            rid,
            ghost: false,
            overflow_page: None,
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
            .find(|entry| !entry.ghost && entry.key == key && entry.sequence <= sequence)
            .map(|entry| entry.rid)
    }

    pub fn mark_ghost(&mut self, key: &[u8], sequence: u64) -> bool {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| !entry.ghost && entry.key == key && entry.sequence <= sequence)
        {
            entry.ghost = true;
            true
        } else {
            false
        }
    }

    pub fn set_overflow_page(
        &mut self,
        key: &[u8],
        sequence: u64,
        overflow_page: BTreePageId,
    ) -> bool {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.key == key && entry.sequence == sequence)
        {
            entry.overflow_page = Some(overflow_page);
            true
        } else {
            false
        }
    }

    pub fn defragment(&mut self) {
        self.entries.retain(|entry| !entry.ghost);
        self.entries.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then(right.sequence.cmp(&left.sequence))
        });
    }

    pub fn slotted_layout(&self) -> BTreeSlottedLayout {
        let header_len = 64;
        let mut offset = header_len;
        let mut slots = Vec::with_capacity(self.entries.len());
        let mut overflow_page_count = 0;
        for entry in &self.entries {
            let len = entry_cell_len(entry);
            if entry.overflow_page.is_some() {
                overflow_page_count += 1;
            }
            slots.push(BTreePageSlot {
                offset,
                len,
                ghost: entry.ghost,
            });
            offset = offset.saturating_add(len);
        }
        let slot_bytes = (slots.len() as u32).saturating_mul(8);
        BTreeSlottedLayout {
            page_size: DEFAULT_PAGE_SIZE,
            header_len,
            free_space_start: offset,
            free_space_end: DEFAULT_PAGE_SIZE.saturating_sub(slot_bytes),
            slot_directory: slots,
            overflow_page_count,
            compressed: self.compressed,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = BinaryWriter::new();
        payload.write_u64(self.page_id.0);
        payload.write_u8(match self.kind {
            BTreePageKind::Leaf => 1,
            BTreePageKind::Internal => 2,
        });
        payload.write_u64(self.page_lsn);
        write_optional_page_id(&mut payload, self.left_sibling);
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
        payload.write_len_bytes(self.low_fence.as_deref().unwrap_or_default());
        payload.write_len_bytes(self.high_fence.as_deref().unwrap_or_default());
        payload.write_u8(u8::from(self.compressed));
        payload.write_u32(self.entries.len() as u32);
        for entry in &self.entries {
            payload.write_len_bytes(&entry.key);
            payload.write_u64(entry.sequence);
            payload.write_u64(entry.rid.0);
            payload.write_u8(u8::from(entry.ghost));
            write_optional_page_id(&mut payload, entry.overflow_page);
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
        let left_sibling = read_optional_page_id(&mut reader, &source)?;
        let right_sibling = match reader.read_u8()? {
            0 => {
                let _ = reader.read_u64()?;
                None
            }
            1 => Some(BTreePageId(reader.read_u64()?)),
            tag => return corrupt(&source, &format!("invalid B-tree sibling tag {tag}")),
        };
        let low_fence = empty_to_none(reader.read_len_bytes()?);
        let high_fence = empty_to_none(reader.read_len_bytes()?);
        let compressed = reader.read_u8()? != 0;
        let count = reader.read_u32()? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(BTreePageEntry {
                key: reader.read_len_bytes()?,
                sequence: reader.read_u64()?,
                rid: RecordId(reader.read_u64()?),
                ghost: reader.read_u8()? != 0,
                overflow_page: read_optional_page_id(&mut reader, &source)?,
            });
        }
        Ok(Self {
            page_id,
            kind,
            page_lsn,
            left_sibling,
            right_sibling,
            low_fence,
            high_fence,
            compressed,
            entries,
        })
    }
}

fn entry_cell_len(entry: &BTreePageEntry) -> u32 {
    4_u32
        .saturating_add(entry.key.len() as u32)
        .saturating_add(8)
        .saturating_add(8)
        .saturating_add(1)
        .saturating_add(9)
}

fn write_optional_page_id(writer: &mut BinaryWriter, page_id: Option<BTreePageId>) {
    match page_id {
        Some(page_id) => {
            writer.write_u8(1);
            writer.write_u64(page_id.0);
        }
        None => {
            writer.write_u8(0);
            writer.write_u64(0);
        }
    }
}

fn read_optional_page_id(
    reader: &mut BinaryReader<'_>,
    source: &Path,
) -> Result<Option<BTreePageId>> {
    match reader.read_u8()? {
        0 => {
            let _ = reader.read_u64()?;
            Ok(None)
        }
        1 => Ok(Some(BTreePageId(reader.read_u64()?))),
        tag => corrupt(source, &format!("invalid B-tree page-id option tag {tag}")),
    }
}

fn empty_to_none(bytes: Vec<u8>) -> Option<Vec<u8>> {
    (!bytes.is_empty()).then_some(bytes)
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(PraxisError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
