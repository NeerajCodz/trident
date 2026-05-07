use super::sstable::{SstableOptions, SstableWriter};
use crate::errors::Result;
use crate::store::RecordId;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemtableEntryKind {
    Put,
    Tombstone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemtableEntry {
    pub key: Vec<u8>,
    pub sequence: u64,
    pub kind: MemtableEntryKind,
    pub rid: Option<RecordId>,
}

#[derive(Clone, Debug, Default)]
pub struct MutableMemtable {
    entries: BTreeMap<Vec<u8>, Vec<MemtableEntry>>,
    approximate_bytes: usize,
}

impl MutableMemtable {
    pub fn put(&mut self, key: impl Into<Vec<u8>>, sequence: u64, rid: RecordId) {
        let key = key.into();
        self.approximate_bytes += key.len() + 24;
        self.entries
            .entry(key.clone())
            .or_default()
            .push(MemtableEntry {
                key,
                sequence,
                kind: MemtableEntryKind::Put,
                rid: Some(rid),
            });
    }

    pub fn delete(&mut self, key: impl Into<Vec<u8>>, sequence: u64) {
        let key = key.into();
        self.approximate_bytes += key.len() + 16;
        self.entries
            .entry(key.clone())
            .or_default()
            .push(MemtableEntry {
                key,
                sequence,
                kind: MemtableEntryKind::Tombstone,
                rid: None,
            });
    }

    pub fn freeze(&mut self) -> ImmutableMemtable {
        let entries = std::mem::take(&mut self.entries);
        let approximate_bytes = std::mem::take(&mut self.approximate_bytes);
        ImmutableMemtable {
            entries,
            approximate_bytes,
        }
    }

    pub fn approximate_bytes(&self) -> usize {
        self.approximate_bytes
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ImmutableMemtable {
    entries: BTreeMap<Vec<u8>, Vec<MemtableEntry>>,
    approximate_bytes: usize,
}

impl ImmutableMemtable {
    pub fn iter_entries(&self) -> impl Iterator<Item = &MemtableEntry> {
        self.entries.values().flat_map(|versions| versions.iter())
    }

    pub fn approximate_bytes(&self) -> usize {
        self.approximate_bytes
    }

    pub fn entry_count(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LsmFlushReport {
    pub path: PathBuf,
    pub entries: u64,
    pub approximate_input_bytes: usize,
    pub sstable_generation: u64,
}

#[derive(Debug)]
pub struct LsmFlushPipeline {
    dir: PathBuf,
    next_generation: u64,
    immutable: VecDeque<ImmutableMemtable>,
}

impl LsmFlushPipeline {
    pub fn new(dir: impl Into<PathBuf>, next_generation: u64) -> Self {
        Self {
            dir: dir.into(),
            next_generation,
            immutable: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, memtable: ImmutableMemtable) {
        if memtable.entry_count() > 0 {
            self.immutable.push_back(memtable);
        }
    }

    pub fn pending_count(&self) -> usize {
        self.immutable.len()
    }

    pub fn flush_next(&mut self) -> Result<Option<LsmFlushReport>> {
        let Some(memtable) = self.immutable.pop_front() else {
            return Ok(None);
        };
        std::fs::create_dir_all(&self.dir)?;
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        let path = self.dir.join(format!("{generation:020}.sst"));
        let mut writer = SstableWriter::create(
            &path,
            SstableOptions {
                generation,
                ..SstableOptions::default()
            },
        );
        for entry in memtable.iter_entries() {
            match entry.kind {
                MemtableEntryKind::Put => {
                    writer.add_put(entry.key.clone(), entry.sequence, entry.rid.unwrap());
                }
                MemtableEntryKind::Tombstone => {
                    writer.add_tombstone(entry.key.clone(), entry.sequence);
                }
            }
        }
        let metadata = writer.finish()?;
        Ok(Some(LsmFlushReport {
            path,
            entries: metadata.entry_count,
            approximate_input_bytes: memtable.approximate_bytes(),
            sstable_generation: generation,
        }))
    }
}
