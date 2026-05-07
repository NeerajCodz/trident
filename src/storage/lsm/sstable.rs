use crate::errors::{Result, TridentError};
use crate::io::{crc32c, BinaryReader, BinaryWriter};
use crate::store::RecordId;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const SSTABLE_MAGIC: u32 = 0x5453_5354;
const SSTABLE_VERSION: u8 = 1;
const FOOTER_MAGIC: u32 = 0x5453_4654;
const HEADER_LEN: usize = 5;
const TRAILER_LEN: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SstableOptions {
    pub level: u32,
    pub generation: u64,
    pub block_target_bytes: usize,
}

impl Default for SstableOptions {
    fn default() -> Self {
        Self {
            level: 0,
            generation: 0,
            block_target_bytes: 16 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SstableMetadata {
    pub level: u32,
    pub generation: u64,
    pub entry_count: u64,
    pub block_count: u32,
    pub smallest_sequence: u64,
    pub largest_sequence: u64,
    pub min_key: Option<Vec<u8>>,
    pub max_key: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SstableBlockIndexEntry {
    pub first_key: Vec<u8>,
    pub last_key: Vec<u8>,
    pub offset: u64,
    pub len: u32,
    pub entry_count: u32,
    pub crc32: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Put,
    Tombstone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SstableEntry {
    key: Vec<u8>,
    sequence: u64,
    kind: EntryKind,
    rid: Option<RecordId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SstableFooter {
    metadata: SstableMetadata,
    index_offset: u64,
    index_len: u32,
    filter_offset: u64,
    filter_len: u32,
}

pub struct SstableWriter {
    path: PathBuf,
    options: SstableOptions,
    entries: Vec<SstableEntry>,
}

impl SstableWriter {
    pub fn create(path: impl Into<PathBuf>, options: SstableOptions) -> Self {
        Self {
            path: path.into(),
            options,
            entries: Vec::new(),
        }
    }

    pub fn add_put(&mut self, key: impl Into<Vec<u8>>, sequence: u64, rid: RecordId) {
        self.entries.push(SstableEntry {
            key: key.into(),
            sequence,
            kind: EntryKind::Put,
            rid: Some(rid),
        });
    }

    pub fn add_tombstone(&mut self, key: impl Into<Vec<u8>>, sequence: u64) {
        self.entries.push(SstableEntry {
            key: key.into(),
            sequence,
            kind: EntryKind::Tombstone,
            rid: None,
        });
    }

    pub fn finish(mut self) -> Result<SstableMetadata> {
        self.entries.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then(right.sequence.cmp(&left.sequence))
        });
        let smallest_sequence = self
            .entries
            .iter()
            .map(|entry| entry.sequence)
            .min()
            .unwrap_or(0);
        let largest_sequence = self
            .entries
            .iter()
            .map(|entry| entry.sequence)
            .max()
            .unwrap_or(0);
        let filter = encode_filter(self.entries.iter().map(|entry| entry.key.as_slice()));

        let mut out = Vec::new();
        out.extend_from_slice(&SSTABLE_MAGIC.to_le_bytes());
        out.push(SSTABLE_VERSION);

        let mut block_index = Vec::new();
        let mut block = Vec::new();
        for entry in self.entries {
            block.push(entry);
            let encoded_len = encode_block(&block).len();
            if encoded_len >= self.options.block_target_bytes.max(128) {
                write_block(&mut out, &mut block_index, &block);
                block.clear();
            }
        }
        if !block.is_empty() {
            write_block(&mut out, &mut block_index, &block);
        }

        let filter_offset = out.len() as u64;
        out.extend_from_slice(&filter);
        let filter_len = filter.len() as u32;

        let index_offset = out.len() as u64;
        let index = encode_index(&block_index);
        out.extend_from_slice(&index);
        let index_len = index.len() as u32;

        let metadata = metadata_from_blocks(
            &self.options,
            &block_index,
            smallest_sequence,
            largest_sequence,
        );
        let footer = SstableFooter {
            metadata: metadata.clone(),
            index_offset,
            index_len,
            filter_offset,
            filter_len,
        };
        let footer = encode_footer(&footer);
        out.extend_from_slice(&footer);
        out.extend_from_slice(&(footer.len() as u32).to_le_bytes());
        out.extend_from_slice(&FOOTER_MAGIC.to_le_bytes());

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, out)?;
        Ok(metadata)
    }
}

pub struct SstableReader {
    path: PathBuf,
    bytes: Vec<u8>,
    metadata: SstableMetadata,
    index: Vec<SstableBlockIndexEntry>,
    filter: BTreeSet<u32>,
}

impl SstableReader {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let bytes = std::fs::read(&path)?;
        if bytes.len() < HEADER_LEN + TRAILER_LEN {
            return corrupt(&path, "truncated SSTable");
        }
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != SSTABLE_MAGIC {
            return corrupt(&path, "bad SSTable magic");
        }
        if bytes[4] != SSTABLE_VERSION {
            return corrupt(&path, &format!("unsupported SSTable version {}", bytes[4]));
        }

        let trailer = bytes.len() - TRAILER_LEN;
        let footer_len = u32::from_le_bytes([
            bytes[trailer],
            bytes[trailer + 1],
            bytes[trailer + 2],
            bytes[trailer + 3],
        ]) as usize;
        let footer_magic = u32::from_le_bytes([
            bytes[trailer + 4],
            bytes[trailer + 5],
            bytes[trailer + 6],
            bytes[trailer + 7],
        ]);
        if footer_magic != FOOTER_MAGIC {
            return corrupt(&path, "bad SSTable footer magic");
        }
        if trailer < footer_len {
            return corrupt(&path, "truncated SSTable footer");
        }
        let footer_start = trailer - footer_len;
        let footer = decode_footer(&bytes[footer_start..trailer], &path)?;

        let filter_start = footer.filter_offset as usize;
        let filter_end = filter_start.saturating_add(footer.filter_len as usize);
        let index_start = footer.index_offset as usize;
        let index_end = index_start.saturating_add(footer.index_len as usize);
        if filter_end > footer_start || index_end > footer_start {
            return corrupt(&path, "SSTable index/filter offset outside file bounds");
        }
        let filter = decode_filter(&bytes[filter_start..filter_end], &path)?;
        let index = decode_index(&bytes[index_start..index_end], &path)?;

        Ok(Self {
            path,
            bytes,
            metadata: footer.metadata,
            index,
            filter,
        })
    }

    pub fn metadata(&self) -> &SstableMetadata {
        &self.metadata
    }

    pub fn may_contain_key(&self, key: &[u8]) -> bool {
        self.filter.contains(&crc32c(key))
    }

    pub fn get_at(&self, key: &[u8], sequence: u64) -> Result<Option<RecordId>> {
        if !self.may_contain_key(key) {
            return Ok(None);
        }
        for block in self
            .index
            .iter()
            .filter(|block| key >= block.first_key.as_slice() && key <= block.last_key.as_slice())
        {
            for entry in self.read_block(block)? {
                if entry.key == key && entry.sequence <= sequence {
                    return Ok(entry.rid);
                }
            }
        }
        Ok(None)
    }

    pub fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<Vec<(Vec<u8>, RecordId)>> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for block in &self.index {
            if start.is_some_and(|s| block.last_key.as_slice() < s)
                || end.is_some_and(|e| block.first_key.as_slice() >= e)
            {
                continue;
            }
            for entry in self.read_block(block)? {
                if !seen.insert(entry.key.clone()) {
                    continue;
                }
                if start.is_some_and(|s| entry.key.as_slice() < s)
                    || end.is_some_and(|e| entry.key.as_slice() >= e)
                {
                    continue;
                }
                if let Some(rid) = entry.rid {
                    out.push((entry.key, rid));
                }
            }
        }
        Ok(out)
    }

    fn read_block(&self, block: &SstableBlockIndexEntry) -> Result<Vec<SstableEntry>> {
        let start = block.offset as usize;
        let end = start.saturating_add(block.len as usize);
        if start < HEADER_LEN || end > self.bytes.len() {
            return corrupt(&self.path, "SSTable block outside file bounds");
        }
        let bytes = &self.bytes[start..end];
        let actual = crc32c(bytes);
        if actual != block.crc32 {
            return corrupt(
                &self.path,
                &format!(
                    "SSTable block checksum mismatch: expected {:#010x}, got {:#010x}",
                    block.crc32, actual
                ),
            );
        }
        decode_block(bytes, &self.path)
    }
}

fn write_block(
    out: &mut Vec<u8>,
    index: &mut Vec<SstableBlockIndexEntry>,
    entries: &[SstableEntry],
) {
    let encoded = encode_block(entries);
    let offset = out.len() as u64;
    out.extend_from_slice(&encoded);
    index.push(SstableBlockIndexEntry {
        first_key: entries
            .first()
            .map(|entry| entry.key.clone())
            .unwrap_or_default(),
        last_key: entries
            .last()
            .map(|entry| entry.key.clone())
            .unwrap_or_default(),
        offset,
        len: encoded.len() as u32,
        entry_count: entries.len() as u32,
        crc32: crc32c(&encoded),
    });
}

fn encode_block(entries: &[SstableEntry]) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(entries.len() as u32);
    for entry in entries {
        writer.write_len_bytes(&entry.key);
        writer.write_u64(entry.sequence);
        match entry.kind {
            EntryKind::Put => {
                writer.write_u8(1);
                writer.write_u64(entry.rid.unwrap_or(RecordId::NULL).0);
            }
            EntryKind::Tombstone => {
                writer.write_u8(0);
                writer.write_u64(0);
            }
        }
    }
    writer.into_inner()
}

fn decode_block(bytes: &[u8], source: &Path) -> Result<Vec<SstableEntry>> {
    let mut reader = BinaryReader::new(bytes, source.to_path_buf());
    let count = reader.read_u32()? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key = reader.read_len_bytes()?;
        let sequence = reader.read_u64()?;
        let kind = match reader.read_u8()? {
            0 => EntryKind::Tombstone,
            1 => EntryKind::Put,
            tag => {
                return corrupt(source, &format!("invalid SSTable entry kind {tag}"));
            }
        };
        let rid = RecordId(reader.read_u64()?);
        entries.push(SstableEntry {
            key,
            sequence,
            kind,
            rid: (kind == EntryKind::Put).then_some(rid),
        });
    }
    Ok(entries)
}

fn encode_index(index: &[SstableBlockIndexEntry]) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(index.len() as u32);
    for block in index {
        writer.write_len_bytes(&block.first_key);
        writer.write_len_bytes(&block.last_key);
        writer.write_u64(block.offset);
        writer.write_u32(block.len);
        writer.write_u32(block.entry_count);
        writer.write_u32(block.crc32);
    }
    writer.into_inner()
}

fn decode_index(bytes: &[u8], source: &Path) -> Result<Vec<SstableBlockIndexEntry>> {
    let mut reader = BinaryReader::new(bytes, source.to_path_buf());
    let count = reader.read_u32()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(SstableBlockIndexEntry {
            first_key: reader.read_len_bytes()?,
            last_key: reader.read_len_bytes()?,
            offset: reader.read_u64()?,
            len: reader.read_u32()?,
            entry_count: reader.read_u32()?,
            crc32: reader.read_u32()?,
        });
    }
    Ok(out)
}

fn encode_filter<'a>(keys: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut hashes = BTreeSet::new();
    for key in keys {
        hashes.insert(crc32c(key));
    }
    let mut writer = BinaryWriter::new();
    writer.write_u32(hashes.len() as u32);
    for hash in hashes {
        writer.write_u32(hash);
    }
    writer.into_inner()
}

fn decode_filter(bytes: &[u8], source: &Path) -> Result<BTreeSet<u32>> {
    let mut reader = BinaryReader::new(bytes, source.to_path_buf());
    let count = reader.read_u32()? as usize;
    let mut out = BTreeSet::new();
    for _ in 0..count {
        out.insert(reader.read_u32()?);
    }
    Ok(out)
}

fn metadata_from_blocks(
    options: &SstableOptions,
    index: &[SstableBlockIndexEntry],
    smallest_sequence: u64,
    largest_sequence: u64,
) -> SstableMetadata {
    SstableMetadata {
        level: options.level,
        generation: options.generation,
        entry_count: index.iter().map(|block| block.entry_count as u64).sum(),
        block_count: index.len() as u32,
        smallest_sequence,
        largest_sequence,
        min_key: index.first().map(|block| block.first_key.clone()),
        max_key: index.last().map(|block| block.last_key.clone()),
    }
}

fn encode_footer(footer: &SstableFooter) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(footer.metadata.level);
    writer.write_u64(footer.metadata.generation);
    writer.write_u64(footer.metadata.entry_count);
    writer.write_u32(footer.metadata.block_count);
    writer.write_u64(footer.metadata.smallest_sequence);
    writer.write_u64(footer.metadata.largest_sequence);
    writer.write_len_bytes(footer.metadata.min_key.as_deref().unwrap_or_default());
    writer.write_len_bytes(footer.metadata.max_key.as_deref().unwrap_or_default());
    writer.write_u64(footer.index_offset);
    writer.write_u32(footer.index_len);
    writer.write_u64(footer.filter_offset);
    writer.write_u32(footer.filter_len);
    let mut bytes = writer.into_inner();
    let crc = crc32c(&bytes);
    bytes.extend_from_slice(&crc.to_le_bytes());
    bytes
}

fn decode_footer(bytes: &[u8], source: &Path) -> Result<SstableFooter> {
    if bytes.len() < 4 {
        return corrupt(source, "truncated SSTable footer checksum");
    }
    let payload_len = bytes.len() - 4;
    let expected = u32::from_le_bytes([
        bytes[payload_len],
        bytes[payload_len + 1],
        bytes[payload_len + 2],
        bytes[payload_len + 3],
    ]);
    let actual = crc32c(&bytes[..payload_len]);
    if actual != expected {
        return corrupt(
            source,
            &format!(
                "SSTable footer checksum mismatch: expected {expected:#010x}, got {actual:#010x}"
            ),
        );
    }
    let mut reader = BinaryReader::new(&bytes[..payload_len], source.to_path_buf());
    let level = reader.read_u32()?;
    let generation = reader.read_u64()?;
    let entry_count = reader.read_u64()?;
    let block_count = reader.read_u32()?;
    let smallest_sequence = reader.read_u64()?;
    let largest_sequence = reader.read_u64()?;
    let min_key = empty_to_none(reader.read_len_bytes()?);
    let max_key = empty_to_none(reader.read_len_bytes()?);
    Ok(SstableFooter {
        metadata: SstableMetadata {
            level,
            generation,
            entry_count,
            block_count,
            smallest_sequence,
            largest_sequence,
            min_key,
            max_key,
        },
        index_offset: reader.read_u64()?,
        index_len: reader.read_u32()?,
        filter_offset: reader.read_u64()?,
        filter_len: reader.read_u32()?,
    })
}

fn empty_to_none(bytes: Vec<u8>) -> Option<Vec<u8>> {
    (!bytes.is_empty()).then_some(bytes)
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(TridentError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
