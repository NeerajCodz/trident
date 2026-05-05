use crate::accel::{Accelerator, CpuAccelerator};
use crate::cache::BlockCache;
use crate::config::{AcceleratorBackend, TridentConfig};
use crate::disk::DiskLayout;
use crate::errors::{Result, TridentError};
use crate::manifest::{Manifest, ManifestStore};
use crate::metrics::EngineMetrics;
use crate::ram::{MemTable, SnapshotManager};
use crate::recovery::RecoveryReport;
use crate::segments::{SegmentReader, SegmentWriter};
use crate::transactions::{BatchOp, WriteBatch};
use crate::types::{ColumnFamily, Key, ReadSnapshot, SequenceNumber, StoredValue, Value, ValueRef};
use crate::wal::{Wal, WalRecord};
use bytes::Bytes;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub struct TridentEngine {
    inner: Arc<EngineInner>,
}

struct EngineInner {
    config: TridentConfig,
    layout: DiskLayout,
    manifest_store: ManifestStore,
    manifest: Mutex<Manifest>,
    wal: Mutex<Wal>,
    memtable: Mutex<MemTable>,
    segment_index: Mutex<BTreeMap<(ColumnFamily, Key), Vec<crate::types::VersionedValue>>>,
    cache: Mutex<BlockCache<(u64, Vec<u8>)>>,
    snapshots: SnapshotManager,
    metrics: EngineMetrics,
    accelerator: Arc<dyn Accelerator>,
}

impl TridentEngine {
    pub fn open(config: TridentConfig) -> Result<Self> {
        config.validate()?;
        let accelerator: Arc<dyn Accelerator> = match config.accelerator {
            AcceleratorBackend::Cpu => Arc::new(CpuAccelerator),
            AcceleratorBackend::Cuda => {
                return Err(TridentError::UnsupportedAccelerator("cuda".to_string()));
            }
            AcceleratorBackend::Vulkan => {
                return Err(TridentError::UnsupportedAccelerator("vulkan".to_string()));
            }
            AcceleratorBackend::Metal => {
                return Err(TridentError::UnsupportedAccelerator("metal".to_string()));
            }
        };
        let layout = DiskLayout::create(&config.data_dir)?;
        let manifest_store = ManifestStore::new(layout.manifest_path());
        let manifest = manifest_store.load_or_create()?;
        let mut segment_index: BTreeMap<(ColumnFamily, Key), Vec<crate::types::VersionedValue>> =
            BTreeMap::new();
        for segment in &manifest.segments {
            let entries =
                SegmentReader::read(std::path::Path::new(&segment.path), accelerator.as_ref())?;
            for entry in entries {
                segment_index
                    .entry((entry.cf, entry.key))
                    .or_default()
                    .push(entry.version);
            }
        }
        let wal_path = layout.wal_path(1);
        let wal_records = Wal::replay(&wal_path)?;
        let mut memtable = MemTable::default();
        let snapshots = SnapshotManager::default();
        for record in &wal_records {
            for op in record.batch.ops() {
                memtable.apply(record.sequence, op);
            }
            snapshots.observe(record.sequence);
        }
        snapshots.observe(manifest.last_sequence);
        let wal = Wal::open(wal_path, config.wal_sync_policy)?;
        let metrics = EngineMetrics::default();
        metrics
            .recovered_records
            .store(wal_records.len() as u64, Ordering::Relaxed);
        Ok(Self {
            inner: Arc::new(EngineInner {
                cache: Mutex::new(BlockCache::new(config.cache_size_bytes)),
                config,
                layout,
                manifest_store,
                manifest: Mutex::new(manifest),
                wal: Mutex::new(wal),
                memtable: Mutex::new(memtable),
                segment_index: Mutex::new(segment_index),
                snapshots,
                metrics,
                accelerator,
            }),
        })
    }

    pub fn put(&self, key: impl Into<Key>, value: impl Into<Value>) -> Result<SequenceNumber> {
        let mut batch = WriteBatch::new();
        batch.put_default(key, value);
        self.write_batch(batch)
    }

    pub fn delete(&self, key: impl Into<Key>) -> Result<SequenceNumber> {
        let mut batch = WriteBatch::new();
        batch.delete_default(key);
        self.write_batch(batch)
    }

    pub fn compare_and_swap(
        &self,
        key: impl Into<Key>,
        expected: Option<&[u8]>,
        value: impl Into<Value>,
    ) -> Result<SequenceNumber> {
        let key = key.into();
        let current = self.get(&key)?;
        let current_bytes = current.as_ref().map(|value| value.as_ref());
        if current_bytes != expected {
            return Err(TridentError::CompareAndSwapFailed);
        }
        self.put(key, value)
    }

    pub fn write_batch(&self, batch: WriteBatch) -> Result<SequenceNumber> {
        if batch.is_empty() {
            return Ok(self.snapshot().sequence);
        }
        let sequence = self.inner.snapshots.next_sequence();
        let record = WalRecord {
            sequence,
            batch: batch.clone(),
        };
        self.inner.wal.lock().append(&record)?;
        {
            let mut memtable = self.inner.memtable.lock();
            for op in batch.ops() {
                memtable.apply(sequence, op);
                match op {
                    BatchOp::Put { .. } => {
                        self.inner.metrics.writes.fetch_add(1, Ordering::Relaxed);
                    }
                    BatchOp::Delete { .. } => {
                        self.inner.metrics.deletes.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        self.inner
            .metrics
            .wal_records
            .fetch_add(1, Ordering::Relaxed);
        self.inner.manifest.lock().last_sequence = sequence;
        self.inner
            .manifest_store
            .save(&self.inner.manifest.lock())?;
        Ok(sequence)
    }

    pub fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Value>> {
        self.get_cf(&ColumnFamily::default(), key.as_ref(), self.snapshot())
    }

    pub fn get_cf(
        &self,
        cf: &ColumnFamily,
        key: &[u8],
        snapshot: ReadSnapshot,
    ) -> Result<Option<Value>> {
        self.inner.metrics.reads.fetch_add(1, Ordering::Relaxed);
        if let Some(value) = self.inner.memtable.lock().get(cf, key, snapshot.sequence) {
            return Ok(match value {
                StoredValue::Put(value) => Some(Bytes::from(value)),
                StoredValue::Delete => None,
            });
        }
        let cache_key = (snapshot.sequence, key.to_vec());
        if let Some(value) = self.inner.cache.lock().get(&cache_key) {
            self.inner
                .metrics
                .cache_hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(Some(value));
        }
        self.inner
            .metrics
            .cache_misses
            .fetch_add(1, Ordering::Relaxed);
        let value = self
            .inner
            .segment_index
            .lock()
            .get(&(cf.clone(), Bytes::copy_from_slice(key)))
            .and_then(|versions| {
                versions
                    .iter()
                    .rev()
                    .find(|version| version.sequence <= snapshot.sequence)
                    .cloned()
            });
        Ok(match value.map(|version| version.value) {
            Some(StoredValue::Put(value)) => {
                let value = Bytes::from(value);
                self.inner.cache.lock().insert(cache_key, value.clone());
                Some(value)
            }
            Some(StoredValue::Delete) | None => None,
        })
    }

    pub fn get_ref(&self, key: impl AsRef<[u8]>) -> Result<Option<ValueRef<'static>>> {
        Ok(self.get(key)?.map(ValueRef::Owned))
    }

    pub fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<(Key, Value)>> {
        let snapshot = self.snapshot();
        let cf = ColumnFamily::default();
        let mut rows = BTreeMap::new();
        for (key, value) in self
            .inner
            .memtable
            .lock()
            .scan(&cf, start, end, snapshot.sequence)
        {
            rows.insert(key, value);
        }
        for ((entry_cf, key), versions) in self.inner.segment_index.lock().iter() {
            if entry_cf != &cf {
                continue;
            }
            if start.is_some_and(|start| key.as_ref() < start) {
                continue;
            }
            if end.is_some_and(|end| key.as_ref() >= end) {
                continue;
            }
            if rows.contains_key(key) {
                continue;
            }
            if let Some(version) = versions
                .iter()
                .rev()
                .find(|version| version.sequence <= snapshot.sequence)
                && let StoredValue::Put(value) = &version.value
            {
                rows.insert(key.clone(), Bytes::from(value.clone()));
            }
        }
        Ok(rows.into_iter().take(limit).collect())
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        self.inner.snapshots.snapshot()
    }

    pub fn flush(&self) -> Result<Option<u64>> {
        let entries = self.inner.memtable.lock().drain_latest();
        if entries.is_empty() {
            return Ok(None);
        }
        let segment_id = {
            let mut manifest = self.inner.manifest.lock();
            let id = manifest.next_segment_id;
            manifest.next_segment_id += 1;
            id
        };
        let path = self.inner.layout.segment_path(0, segment_id);
        let segment_entries = entries
            .into_iter()
            .map(|(cf, key, version)| crate::segments::block::SegmentEntry { cf, key, version })
            .collect::<Vec<_>>();
        let metadata = SegmentWriter::write(
            &path,
            segment_id,
            0,
            self.inner.config.compression,
            self.inner.accelerator.as_ref(),
            segment_entries,
        )?;
        let loaded = SegmentReader::read(&path, self.inner.accelerator.as_ref())?;
        {
            let mut segment_index = self.inner.segment_index.lock();
            for entry in loaded {
                segment_index
                    .entry((entry.cf, entry.key))
                    .or_default()
                    .push(entry.version);
            }
        }
        {
            let mut manifest = self.inner.manifest.lock();
            manifest.segments.push(metadata);
            self.inner.manifest_store.save(&manifest)?;
        }
        self.inner.memtable.lock().clear();
        self.inner.wal.lock().truncate()?;
        self.inner
            .metrics
            .segment_flushes
            .fetch_add(1, Ordering::Relaxed);
        Ok(Some(segment_id))
    }

    pub fn compact(&self) -> Result<u64> {
        self.flush()?;
        Ok(self.inner.manifest.lock().segments.len() as u64)
    }

    pub fn recover(&self) -> RecoveryReport {
        RecoveryReport {
            wal_records_replayed: self.inner.metrics.recovered_records.load(Ordering::Relaxed),
            last_sequence: self.snapshot().sequence,
            segment_count: self.inner.manifest.lock().segments.len() as u64,
        }
    }

    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "accelerator": self.inner.accelerator.name(),
            "metrics": self.inner.metrics.snapshot(),
            "manifest": &*self.inner.manifest.lock(),
            "data_dir": self.inner.layout.root(),
        })
    }
}

impl Clone for TridentEngine {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
