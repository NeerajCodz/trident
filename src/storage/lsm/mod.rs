//! LSM-style index: `encoded_key → RecordId`.
//!
//! This implementation stores versioned key histories in memory and on disk:
//! each key has an ordered sequence of updates (`put` or tombstone delete).
//! Snapshot reads resolve the newest update whose sequence is `<= snapshot`.

pub mod binary;
pub mod pipeline;
pub mod sstable;

pub use pipeline::{
    ImmutableMemtable, LsmFlushPipeline, LsmFlushReport, MemtableEntry, MemtableEntryKind,
    MutableMemtable,
};

use crate::errors::{Result, TridentError};
use crate::index::{IndexPlugin, IndexStats};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct VersionedRid {
    sequence: u64,
    rid: Option<RecordId>,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    next_sequence: u64,
    entries: Vec<(Vec<u8>, Vec<VersionedRid>)>,
    #[serde(default)]
    bloom_bits: Vec<u8>,
    #[serde(default)]
    bloom_bit_len: usize,
    #[serde(default)]
    bloom_hashes: u8,
    // Legacy field (exact-set scaffold snapshot format).
    #[serde(default)]
    bloom_keys: Vec<Vec<u8>>,
    #[serde(default)]
    fence_min: Option<Vec<u8>>,
    #[serde(default)]
    fence_max: Option<Vec<u8>>,
}

const LSM_SNAPSHOT_MAGIC: u32 = 0x4c53_4d32;
const LSM_SNAPSHOT_VERSION: u8 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BloomMetadata {
    bits: Vec<u8>,
    bit_len: usize,
    hashes: u8,
}

impl BloomMetadata {
    fn from_keys(keys: &[Vec<u8>], false_positive_rate: f64) -> Self {
        if keys.is_empty() {
            return Self {
                bits: Vec::new(),
                bit_len: 0,
                hashes: 0,
            };
        }
        let n = keys.len() as f64;
        let p = false_positive_rate.clamp(1e-9, 0.5);
        let ln2 = std::f64::consts::LN_2;
        let bit_len = ((-n * p.ln()) / (ln2 * ln2)).ceil().max(1024.0) as usize;
        let hashes = ((bit_len as f64 / n) * ln2).round().clamp(1.0, 16.0) as u8;
        let mut bloom = Self {
            bits: vec![0u8; bit_len.div_ceil(8)],
            bit_len,
            hashes,
        };
        for key in keys {
            bloom.insert(key);
        }
        bloom
    }

    fn maybe_contains(&self, key: &[u8]) -> bool {
        if self.bit_len == 0 || self.hashes == 0 {
            return false;
        }
        for idx in hash_indexes(key, self.hashes, self.bit_len) {
            if !bit_is_set(&self.bits, idx) {
                return false;
            }
        }
        true
    }

    fn insert(&mut self, key: &[u8]) {
        if self.bit_len == 0 || self.hashes == 0 {
            return;
        }
        for idx in hash_indexes(key, self.hashes, self.bit_len) {
            set_bit(&mut self.bits, idx);
        }
    }
}

/// LSM-style key-to-RID index with sequence-aware snapshot lookups.
pub struct LsmIndex {
    name: String,
    dir: PathBuf,
    history: BTreeMap<Vec<u8>, Vec<VersionedRid>>,
    next_sequence: u64,
    bloom: BloomMetadata,
    fence_min: Option<Vec<u8>>,
    fence_max: Option<Vec<u8>>,
}

impl LsmIndex {
    /// Open or create an LSM index named `name` inside `dir`.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let mut history: BTreeMap<Vec<u8>, Vec<VersionedRid>> = BTreeMap::new();
        let mut next_sequence = 0;
        let snapshot = Self::snapshot_path(&dir, &name);
        if snapshot.exists() {
            let bytes = std::fs::read(&snapshot)?;
            let on_disk: OnDisk = if looks_like_binary_snapshot(&bytes) {
                decode_binary_snapshot(&bytes, &snapshot)?
            } else {
                serde_json::from_slice(&bytes)?
            };
            next_sequence = on_disk.next_sequence;
            history.extend(on_disk.entries);
            let mut index = Self {
                name,
                dir,
                history,
                next_sequence,
                bloom: if on_disk.bloom_bit_len > 0 && on_disk.bloom_hashes > 0 {
                    BloomMetadata {
                        bits: on_disk.bloom_bits,
                        bit_len: on_disk.bloom_bit_len,
                        hashes: on_disk.bloom_hashes,
                    }
                } else {
                    BloomMetadata::from_keys(&on_disk.bloom_keys, DEFAULT_BLOOM_FPR)
                },
                fence_min: on_disk.fence_min,
                fence_max: on_disk.fence_max,
            };
            index.refresh_metadata();
            return Ok(index);
        }

        let mut index = Self {
            name,
            dir,
            history,
            next_sequence,
            bloom: BloomMetadata {
                bits: Vec::new(),
                bit_len: 0,
                hashes: 0,
            },
            fence_min: None,
            fence_max: None,
        };
        index.refresh_metadata();
        Ok(index)
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.lsm"))
    }

    /// Forward range scan: returns all live `(key, rid)` pairs in `[start, end)`.
    pub fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Vec<(Vec<u8>, RecordId)> {
        self.history
            .iter()
            .filter(|(key, _)| {
                start.is_none_or(|s| key.as_slice() >= s) && end.is_none_or(|e| key.as_slice() < e)
            })
            .filter_map(|(key, versions)| {
                latest_visible(versions, u64::MAX).map(|rid| (key.clone(), rid))
            })
            .collect()
    }

    fn next_seq(&mut self) -> u64 {
        self.next_sequence += 1;
        self.next_sequence
    }

    /// Probabilistic bloom membership check for live keys.
    pub fn may_contain_key(&self, key: &[u8]) -> bool {
        self.bloom.maybe_contains(key)
    }

    /// Fence pointer bounds `(min_key, max_key)` for live keys.
    pub fn fence_bounds(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        (self.fence_min.clone(), self.fence_max.clone())
    }

    fn refresh_metadata(&mut self) {
        self.fence_min = None;
        self.fence_max = None;
        let mut live_keys = Vec::new();
        for (key, versions) in &self.history {
            if latest_visible(versions, u64::MAX).is_some() {
                live_keys.push(key.clone());
                if self.fence_min.as_ref().is_none_or(|min| key < min) {
                    self.fence_min = Some(key.clone());
                }
                if self.fence_max.as_ref().is_none_or(|max| key > max) {
                    self.fence_max = Some(key.clone());
                }
            }
        }
        self.bloom = BloomMetadata::from_keys(&live_keys, DEFAULT_BLOOM_FPR);
    }
}

impl IndexPlugin for LsmIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        let sequence = self.next_seq();
        self.put_with_sequence(key, rid, sequence)
    }

    fn put_with_sequence(&mut self, key: &[u8], rid: RecordId, sequence: u64) -> Result<()> {
        self.next_sequence = self.next_sequence.max(sequence);
        self.history
            .entry(key.to_vec())
            .or_default()
            .push(VersionedRid {
                sequence,
                rid: Some(rid),
            });
        self.refresh_metadata();
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.get_at(key, u64::MAX)
    }

    fn get_at(&self, key: &[u8], sequence: u64) -> Option<RecordId> {
        self.history
            .get(key)
            .and_then(|versions| latest_visible(versions, sequence))
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let sequence = self.next_seq();
        self.delete_with_sequence(key, sequence)
    }

    fn delete_with_sequence(&mut self, key: &[u8], sequence: u64) -> Result<()> {
        self.next_sequence = self.next_sequence.max(sequence);
        self.history
            .entry(key.to_vec())
            .or_default()
            .push(VersionedRid {
                sequence,
                rid: None,
            });
        self.refresh_metadata();
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let on_disk = OnDisk {
            next_sequence: self.next_sequence,
            entries: self
                .history
                .iter()
                .map(|(key, versions)| (key.clone(), versions.clone()))
                .collect(),
            bloom_bits: self.bloom.bits.clone(),
            bloom_bit_len: self.bloom.bit_len,
            bloom_hashes: self.bloom.hashes,
            bloom_keys: Vec::new(),
            fence_min: self.fence_min.clone(),
            fence_max: self.fence_max.clone(),
        };
        let bytes = encode_binary_snapshot(&on_disk);
        std::fs::write(Self::snapshot_path(&self.dir, &self.name), bytes)?;
        Ok(())
    }

    fn compact(&mut self) -> Result<()> {
        for versions in self.history.values_mut() {
            if let Some(last) = versions.last().copied() {
                versions.clear();
                versions.push(last);
            }
        }
        self.history.retain(|_, versions| versions[0].rid.is_some());
        self.refresh_metadata();
        Ok(())
    }

    fn stats(&self) -> IndexStats {
        let live_keys = self
            .history
            .values()
            .filter(|versions| latest_visible(versions, u64::MAX).is_some())
            .count() as u64;
        let versions = self.history.values().map(|v| v.len() as u64).sum();
        IndexStats {
            live_keys,
            versions,
        }
    }
}

fn latest_visible(versions: &[VersionedRid], sequence: u64) -> Option<RecordId> {
    versions
        .iter()
        .rev()
        .find(|version| version.sequence <= sequence)
        .and_then(|version| version.rid)
}

const DEFAULT_BLOOM_FPR: f64 = 0.01;

fn hash_indexes(key: &[u8], hashes: u8, bit_len: usize) -> impl Iterator<Item = usize> + '_ {
    let digest = blake3::hash(key);
    let bytes = digest.as_bytes();
    let h1 = u64::from_le_bytes(bytes[0..8].try_into().unwrap_or([0u8; 8]));
    let mut h2 = u64::from_le_bytes(bytes[8..16].try_into().unwrap_or([0u8; 8]));
    if h2 == 0 {
        h2 = 0x9e37_79b9_7f4a_7c15;
    }
    (0..hashes).map(move |i| {
        let mixed = h1.wrapping_add((i as u64).wrapping_mul(h2));
        (mixed % bit_len as u64) as usize
    })
}

fn bit_is_set(bits: &[u8], idx: usize) -> bool {
    let byte_idx = idx / 8;
    let bit_idx = idx % 8;
    bits.get(byte_idx)
        .is_some_and(|byte| (byte & (1u8 << bit_idx)) != 0)
}

fn set_bit(bits: &mut [u8], idx: usize) {
    let byte_idx = idx / 8;
    let bit_idx = idx % 8;
    if let Some(byte) = bits.get_mut(byte_idx) {
        *byte |= 1u8 << bit_idx;
    }
}

fn looks_like_binary_snapshot(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == LSM_SNAPSHOT_MAGIC
}

fn encode_binary_snapshot(on_disk: &OnDisk) -> Vec<u8> {
    let mut payload = BinaryWriter::new();
    payload.write_u64(on_disk.next_sequence);
    payload.write_u32(on_disk.entries.len() as u32);
    for (key, versions) in &on_disk.entries {
        payload.write_len_bytes(key);
        payload.write_u32(versions.len() as u32);
        for version in versions {
            payload.write_u64(version.sequence);
            match version.rid {
                Some(rid) => {
                    payload.write_u8(1);
                    payload.write_u64(rid.0);
                }
                None => payload.write_u8(0),
            }
        }
    }
    payload.write_len_bytes(&on_disk.bloom_bits);
    payload.write_u64(on_disk.bloom_bit_len as u64);
    payload.write_u8(on_disk.bloom_hashes);
    write_optional_bytes(&mut payload, on_disk.fence_min.as_ref());
    write_optional_bytes(&mut payload, on_disk.fence_max.as_ref());

    let payload = payload.into_inner();
    let mut out = BinaryWriter::new();
    out.write_u32(LSM_SNAPSHOT_MAGIC);
    out.write_u8(LSM_SNAPSHOT_VERSION);
    out.write_u32(payload.len() as u32);
    out.write_u32(crc32c(&payload));
    out.write_bytes(&payload);
    out.into_inner()
}

fn decode_binary_snapshot(bytes: &[u8], source: &Path) -> Result<OnDisk> {
    if bytes.len() < 13 {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated LSM snapshot header".to_string(),
        });
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != LSM_SNAPSHOT_MAGIC {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "bad LSM snapshot magic".to_string(),
        });
    }
    if bytes[4] != LSM_SNAPSHOT_VERSION {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!("unsupported LSM snapshot version {}", bytes[4]),
        });
    }
    let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    let expected_crc = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    if bytes.len() < 13 + payload_len {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated LSM snapshot payload".to_string(),
        });
    }
    let payload = &bytes[13..13 + payload_len];
    let actual_crc = crc32c(payload);
    if actual_crc != expected_crc {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!(
                "LSM snapshot checksum mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
            ),
        });
    }

    let mut reader = BinaryReader::new(payload, source.to_path_buf());
    let next_sequence = reader.read_u64()?;
    let entry_count = reader.read_u32()? as usize;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let key = reader.read_len_bytes()?;
        let version_count = reader.read_u32()? as usize;
        let mut versions = Vec::with_capacity(version_count);
        for _ in 0..version_count {
            let sequence = reader.read_u64()?;
            let has_rid = reader.read_u8()?;
            let rid = if has_rid == 1 {
                Some(RecordId(reader.read_u64()?))
            } else {
                None
            };
            versions.push(VersionedRid { sequence, rid });
        }
        entries.push((key, versions));
    }
    let bloom_bits = reader.read_len_bytes()?;
    let bloom_bit_len = reader.read_u64()? as usize;
    let bloom_hashes = reader.read_u8()?;
    let fence_min = read_optional_bytes(&mut reader)?;
    let fence_max = read_optional_bytes(&mut reader)?;

    Ok(OnDisk {
        next_sequence,
        entries,
        bloom_bits,
        bloom_bit_len,
        bloom_hashes,
        bloom_keys: Vec::new(),
        fence_min,
        fence_max,
    })
}

fn write_optional_bytes(writer: &mut BinaryWriter, value: Option<&Vec<u8>>) {
    match value {
        Some(bytes) => {
            writer.write_u8(1);
            writer.write_len_bytes(bytes);
        }
        None => writer.write_u8(0),
    }
}

fn read_optional_bytes(reader: &mut BinaryReader<'_>) -> Result<Option<Vec<u8>>> {
    let tag = reader.read_u8()?;
    match tag {
        0 => Ok(None),
        1 => Ok(Some(reader.read_len_bytes()?)),
        _ => Err(TridentError::Corrupt {
            path: PathBuf::from("lsm-snapshot"),
            reason: format!("invalid optional-bytes tag {tag}"),
        }),
    }
}
