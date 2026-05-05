use crate::accel::{Accelerator, CpuAccelerator};
use crate::cache::BlockCache;
use crate::config::{AcceleratorBackend, PersistedEngineConfig, TridentConfig};
use crate::disk::DiskLayout;
use crate::errors::{Result, TridentError};
use crate::manifest::{
    CheckpointMetadata, ColumnFamilyDescriptor, ColumnFamilyOptions, Manifest, ManifestStore,
};
use crate::metrics::EngineMetrics;
use crate::ram::{MemTable, PinnedSnapshot, SnapshotManager};
use crate::recovery::{GcReport, RecoveryReport, write_checkpoint};
use crate::segments::bloom::bloom_key;
use crate::segments::{SegmentReader, SegmentWriteOptions, SegmentWriter};
use crate::slog;
use crate::transactions::OptimisticTransaction;
use crate::transactions::{BatchOp, WriteBatch};
use crate::types::{
    ColumnFamily, Key, ReadSnapshot, SequenceNumber, StoredValue, Value, ValueRef, VersionedValue,
};
use crate::values::ValueLog;
use crate::wal::{Wal, WalRecord};
use bytes::Bytes;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

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
    snapshots: Arc<SnapshotManager>,
    metrics: EngineMetrics,
    accelerator: Arc<dyn Accelerator>,
}

impl TridentEngine {
    pub fn open(config: TridentConfig) -> Result<Self> {
        config.validate()?;
        slog::Logger::init(config.logging.clone());
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
        let mut manifest = manifest_store.load_or_create(&config)?;
        if manifest.effective_config != config.persisted() {
            return Err(TridentError::ConfigMismatch(
                "open config differs from persisted effective engine config".to_string(),
            ));
        }
        if manifest.column_families.is_empty() {
            manifest
                .column_families
                .push(ColumnFamilyDescriptor::default());
            manifest_store.save(&manifest)?;
        }
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
        let wal_records = Wal::replay_dir(&layout.wal_root())?;
        let mut memtable = MemTable::default();
        let snapshots = Arc::new(SnapshotManager::default());
        for record in &wal_records {
            for op in record.batch.ops() {
                memtable.apply(record.sequence, op);
            }
            snapshots.observe(record.sequence);
        }
        snapshots.observe(manifest.last_sequence);
        let wal = Wal::open(
            layout.wal_path(manifest.active_wal_id),
            manifest.active_wal_id,
            config.wal_sync_policy,
        )?;
        let metrics = EngineMetrics::default();
        metrics
            .recovered_records
            .store(wal_records.len() as u64, Ordering::Relaxed);
        let engine = Self {
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
        };
        slog::info(
            "engine_open",
            slog::context().with_str("outcome", "success").with_str(
                "data_dir",
                engine.inner.layout.root().to_string_lossy().to_string(),
            ),
        );
        Ok(engine)
    }

    pub fn open_from_file(path: impl Into<PathBuf>) -> Result<Self> {
        Self::open(TridentConfig::from_file(path)?)
    }

    pub fn effective_config(&self) -> PersistedEngineConfig {
        self.inner.manifest.lock().effective_config.clone()
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

    pub fn put_with_ttl(
        &self,
        key: impl Into<Key>,
        value: impl Into<Value>,
        ttl_seconds: u64,
    ) -> Result<SequenceNumber> {
        let mut batch = WriteBatch::new();
        let expires_at_ms = now_millis().saturating_add(ttl_seconds.saturating_mul(1000));
        batch.put_with_expiry(ColumnFamily::default(), key, value, expires_at_ms);
        self.write_batch(batch)
    }

    pub fn merge(
        &self,
        cf: impl Into<ColumnFamily>,
        key: impl Into<Key>,
        value: impl Into<Value>,
    ) -> Result<SequenceNumber> {
        let mut batch = WriteBatch::new();
        batch.merge(cf, key, value);
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

    pub fn begin_transaction(&self) -> OptimisticTransaction {
        OptimisticTransaction::new(self.clone(), self.snapshot())
    }

    pub(crate) fn commit_optimistic_transaction(
        &self,
        snapshot: ReadSnapshot,
        batch: WriteBatch,
    ) -> Result<SequenceNumber> {
        self.validate_transaction_conflicts(snapshot, &batch)?;
        self.write_batch(batch)
    }

    pub fn write_batch(&self, batch: WriteBatch) -> Result<SequenceNumber> {
        let started = std::time::Instant::now();
        if batch.is_empty() {
            return Ok(self.snapshot().sequence);
        }
        self.validate_batch_column_families(&batch)?;
        let batch = self.normalize_batch_with_cf_policies(batch)?;
        self.relieve_l0_pressure_before_write()?;
        let sequence = self.inner.snapshots.next_sequence();
        let record = WalRecord {
            sequence,
            batch: batch.clone(),
        };
        self.rotate_wal_if_needed(record.encoded_len())?;
        self.inner.wal.lock().append(&record)?;
        {
            let mut memtable = self.inner.memtable.lock();
            for op in batch.ops() {
                memtable.apply(sequence, op);
                match op {
                    BatchOp::Put { .. } | BatchOp::PutWithExpiry { .. } | BatchOp::Merge { .. } => {
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
        let memtable_bytes = self.inner.memtable.lock().approximate_bytes();
        self.inner
            .metrics
            .memtable_bytes
            .store(memtable_bytes as u64, Ordering::Relaxed);
        if memtable_bytes >= self.inner.config.memtable_flush_threshold_bytes {
            self.flush()?;
            self.inner
                .metrics
                .automatic_flushes
                .fetch_add(1, Ordering::Relaxed);
        }
        slog::info(
            "write_batch",
            slog::context()
                .with_u64("seq", sequence)
                .with_u64("ops", batch.len() as u64)
                .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                .with_str("outcome", "success"),
        );
        Ok(sequence)
    }

    fn normalize_batch_with_cf_policies(&self, batch: WriteBatch) -> Result<WriteBatch> {
        let options = self.cf_options_map();
        let mut normalized = WriteBatch::new();
        for op in batch.ops() {
            match op {
                BatchOp::Put { cf, key, value } => {
                    if let Some(ttl_seconds) = options.get(&cf.0).and_then(|cf| cf.ttl_seconds) {
                        let expires_at_ms = now_millis().saturating_add(ttl_seconds * 1000);
                        normalized.put_with_expiry(
                            cf.clone(),
                            Bytes::copy_from_slice(key),
                            Bytes::copy_from_slice(value),
                            expires_at_ms,
                        );
                    } else {
                        normalized.put(
                            cf.clone(),
                            Bytes::copy_from_slice(key),
                            Bytes::copy_from_slice(value),
                        );
                    }
                }
                BatchOp::PutWithExpiry {
                    cf,
                    key,
                    value,
                    expires_at_ms,
                } => {
                    normalized.put_with_expiry(
                        cf.clone(),
                        Bytes::copy_from_slice(key),
                        Bytes::copy_from_slice(value),
                        *expires_at_ms,
                    );
                }
                BatchOp::Merge { cf, key, value } => {
                    normalized.merge(
                        cf.clone(),
                        Bytes::copy_from_slice(key),
                        Bytes::copy_from_slice(value),
                    );
                }
                BatchOp::Delete { cf, key } => {
                    normalized.delete(cf.clone(), Bytes::copy_from_slice(key));
                }
            };
        }
        Ok(normalized)
    }

    fn rotate_wal_if_needed(&self, next_record_len: usize) -> Result<()> {
        let should_rotate = self
            .inner
            .wal
            .lock()
            .should_rotate(next_record_len, self.inner.config.wal_segment_size);
        if !should_rotate {
            return Ok(());
        }
        self.install_new_active_wal()
    }

    fn install_new_active_wal(&self) -> Result<()> {
        let new_wal_id = {
            let mut manifest = self.inner.manifest.lock();
            let id = manifest.next_wal_id;
            manifest.active_wal_id = id;
            manifest.next_wal_id += 1;
            self.inner.manifest_store.save(&manifest)?;
            id
        };
        let new_wal = Wal::open(
            self.inner.layout.wal_path(new_wal_id),
            new_wal_id,
            self.inner.config.wal_sync_policy,
        )?;
        *self.inner.wal.lock() = new_wal;
        Ok(())
    }

    fn relieve_l0_pressure_before_write(&self) -> Result<()> {
        let l0_count = self.l0_segment_count();
        if l0_count < self.inner.config.l0_slowdown_segments {
            return Ok(());
        }
        self.inner
            .metrics
            .write_stalls
            .fetch_add(1, Ordering::Relaxed);
        self.compact()?;
        self.inner
            .metrics
            .l0_pressure_compactions
            .fetch_add(1, Ordering::Relaxed);
        let l0_count = self.l0_segment_count();
        if l0_count >= self.inner.config.l0_stop_segments {
            return Err(TridentError::WriteStalled {
                reason: format!(
                    "level-0 segment count {l0_count} exceeds hard stop {}",
                    self.inner.config.l0_stop_segments
                ),
            });
        }
        Ok(())
    }

    fn l0_segment_count(&self) -> usize {
        self.inner
            .manifest
            .lock()
            .segments
            .iter()
            .filter(|segment| segment.level == 0)
            .count()
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
        self.ensure_column_family(cf)?;
        self.inner.metrics.reads.fetch_add(1, Ordering::Relaxed);
        if let Some(value) = self.resolve_value_at_snapshot(cf, key, snapshot.sequence)? {
            return Ok(Some(Bytes::from(value)));
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
        if !self.segment_filters_may_contain(cf, key) {
            self.inner
                .metrics
                .bloom_negative_hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }
        let resolved = self.resolve_value_at_snapshot(cf, key, snapshot.sequence)?;
        Ok(match resolved {
            Some(value) => {
                let value = Bytes::from(value);
                self.inner.cache.lock().insert(cache_key, value.clone());
                Some(value)
            }
            None => None,
        })
    }

    pub fn create_column_family(&self, mut descriptor: ColumnFamilyDescriptor) -> Result<()> {
        if descriptor.name == ColumnFamily::default().0 {
            return Ok(());
        }
        if descriptor.options == ColumnFamilyOptions::default() {
            descriptor.options.memtable_kind = self.inner.config.default_memtable_kind;
            descriptor.options.compaction_strategy = self.inner.config.default_compaction_strategy;
        }
        let mut manifest = self.inner.manifest.lock();
        if manifest
            .column_families
            .iter()
            .any(|existing| existing.name == descriptor.name)
        {
            return Err(TridentError::ColumnFamilyExists(descriptor.name));
        }
        manifest.column_families.push(descriptor);
        self.inner.manifest_store.save(&manifest)
    }

    pub fn drop_column_family(&self, name: &str) -> Result<()> {
        if name == ColumnFamily::default().0 {
            return Err(TridentError::CannotDropDefaultColumnFamily);
        }
        let mut manifest = self.inner.manifest.lock();
        let original_len = manifest.column_families.len();
        manifest
            .column_families
            .retain(|family| family.name != name);
        if manifest.column_families.len() == original_len {
            return Err(TridentError::UnknownColumnFamily(name.to_string()));
        }
        self.inner.manifest_store.save(&manifest)
    }

    pub fn list_column_families(&self) -> Vec<ColumnFamilyDescriptor> {
        self.inner.manifest.lock().column_families.clone()
    }

    pub fn column_family(&self, name: &str) -> Result<ColumnFamily> {
        let cf = ColumnFamily(name.to_string());
        self.ensure_column_family(&cf)?;
        Ok(cf)
    }

    fn validate_batch_column_families(&self, batch: &WriteBatch) -> Result<()> {
        for op in batch.ops() {
            let cf = match op {
                BatchOp::Put { cf, .. }
                | BatchOp::PutWithExpiry { cf, .. }
                | BatchOp::Merge { cf, .. }
                | BatchOp::Delete { cf, .. } => cf,
            };
            self.ensure_column_family(cf)?;
        }
        Ok(())
    }

    fn validate_transaction_conflicts(
        &self,
        snapshot: ReadSnapshot,
        batch: &WriteBatch,
    ) -> Result<()> {
        for op in batch.ops() {
            let (cf, key) = match op {
                BatchOp::Put { cf, key, .. }
                | BatchOp::PutWithExpiry { cf, key, .. }
                | BatchOp::Merge { cf, key, .. }
                | BatchOp::Delete { cf, key } => (cf, key),
            };
            if self.latest_sequence_for_key(cf, key)? > snapshot.sequence {
                return Err(TridentError::TransactionConflict {
                    cf: cf.0.clone(),
                    key: String::from_utf8_lossy(key).to_string(),
                });
            }
        }
        Ok(())
    }

    fn latest_sequence_for_key(&self, cf: &ColumnFamily, key: &[u8]) -> Result<SequenceNumber> {
        self.ensure_column_family(cf)?;
        let memtable_sequence = self
            .inner
            .memtable
            .lock()
            .latest_sequence(cf, key)
            .unwrap_or_default();
        let segment_sequence = self
            .inner
            .segment_index
            .lock()
            .get(&(cf.clone(), Bytes::copy_from_slice(key)))
            .and_then(|versions| versions.last())
            .map(|version| version.sequence)
            .unwrap_or_default();
        Ok(memtable_sequence.max(segment_sequence))
    }

    fn ensure_column_family(&self, cf: &ColumnFamily) -> Result<()> {
        if self
            .inner
            .manifest
            .lock()
            .column_families
            .iter()
            .any(|family| family.name == cf.0)
        {
            Ok(())
        } else {
            Err(TridentError::UnknownColumnFamily(cf.0.clone()))
        }
    }

    pub fn get_ref(&self, key: impl AsRef<[u8]>) -> Result<Option<ValueRef<'static>>> {
        Ok(self.get(key)?.map(ValueRef::Owned))
    }

    fn segment_filters_may_contain(&self, cf: &ColumnFamily, key: &[u8]) -> bool {
        let encoded = bloom_key(&cf.0, key);
        self.inner.manifest.lock().segments.iter().any(|segment| {
            if !segment.min_key.is_empty()
                && (key < segment.min_key.as_slice() || key > segment.max_key.as_slice())
            {
                return false;
            }
            segment.bloom_filter.may_contain(&encoded)
        })
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
            if let Some(value) = self.resolve_versions_chain(&cf, versions, snapshot.sequence)? {
                rows.insert(key.clone(), Bytes::from(value));
            }
        }
        Ok(rows.into_iter().take(limit).collect())
    }

    pub fn scan_prefix(&self, prefix: &[u8], limit: usize) -> Result<Vec<(Key, Value)>> {
        let end = prefix_upper_bound(prefix);
        self.scan(Some(prefix), end.as_deref(), limit)
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        self.inner.snapshots.snapshot()
    }

    pub fn pin_snapshot(&self) -> PinnedSnapshot {
        self.inner.snapshots.pin()
    }

    pub fn flush(&self) -> Result<Option<u64>> {
        let started = std::time::Instant::now();
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
        let mut value_log = ValueLog::open(self.inner.layout.value_log_path(segment_id))?;
        let segment_entries = entries
            .into_iter()
            .map(|(cf, key, version)| crate::segments::block::SegmentEntry { cf, key, version })
            .collect::<Vec<_>>();
        let metadata = SegmentWriter::write(
            SegmentWriteOptions {
                path: &path,
                id: segment_id,
                level: 0,
                compression: self.inner.config.compression,
                accelerator: self.inner.accelerator.as_ref(),
                value_log: &mut value_log,
                large_value_threshold: self.inner.config.large_value_threshold,
                block_size: self.inner.config.block_size,
            },
            segment_entries,
        )?;
        let flushed_entries = metadata.entries;
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
        self.inner
            .metrics
            .memtable_bytes
            .store(0, Ordering::Relaxed);
        self.install_new_active_wal()?;
        self.inner
            .metrics
            .segment_flushes
            .fetch_add(1, Ordering::Relaxed);
        slog::info(
            "flush",
            slog::context()
                .with_u64("segment_id", segment_id)
                .with_u64("entries", flushed_entries)
                .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                .with_str("outcome", "success"),
        );
        Ok(Some(segment_id))
    }

    pub fn compact(&self) -> Result<u64> {
        let started = std::time::Instant::now();
        self.flush()?;
        let latest_sequence = self.snapshot().sequence;
        let oldest_pinned = self.inner.snapshots.oldest_pinned_sequence();
        let mut compacted = Vec::new();
        let cf_options = self.cf_options_map();
        for ((cf, key), versions) in self.inner.segment_index.lock().iter() {
            let strategy = cf_options
                .get(&cf.0)
                .map(|options| options.compaction_strategy)
                .unwrap_or(self.inner.config.default_compaction_strategy);
            compacted.extend(self.compaction_retained_versions(
                cf,
                key,
                versions,
                latest_sequence,
                oldest_pinned,
                strategy,
            ));
        }
        if compacted.is_empty() {
            let mut manifest = self.inner.manifest.lock();
            manifest.segments.clear();
            self.inner.manifest_store.save(&manifest)?;
            self.inner.segment_index.lock().clear();
            return Ok(0);
        }
        let segment_id = {
            let mut manifest = self.inner.manifest.lock();
            let id = manifest.next_segment_id;
            manifest.next_segment_id += 1;
            id
        };
        let path = self.inner.layout.segment_path(1, segment_id);
        std::fs::create_dir_all(path.parent().expect("segment path has parent"))?;
        let mut value_log = ValueLog::open(self.inner.layout.value_log_path(segment_id))?;
        let metadata = SegmentWriter::write(
            SegmentWriteOptions {
                path: &path,
                id: segment_id,
                level: 1,
                compression: self.inner.config.compression,
                accelerator: self.inner.accelerator.as_ref(),
                value_log: &mut value_log,
                large_value_threshold: self.inner.config.large_value_threshold,
                block_size: self.inner.config.block_size,
            },
            compacted,
        )?;
        let compacted_entries = metadata.entries;
        let loaded = SegmentReader::read(&path, self.inner.accelerator.as_ref())?;
        let mut rebuilt = BTreeMap::new();
        for entry in loaded {
            rebuilt
                .entry((entry.cf, entry.key))
                .or_insert_with(Vec::new)
                .push(entry.version);
        }
        {
            let mut segment_index = self.inner.segment_index.lock();
            *segment_index = rebuilt;
        }
        {
            let mut manifest = self.inner.manifest.lock();
            manifest.segments.clear();
            manifest.segments.push(metadata);
            self.inner.manifest_store.save(&manifest)?;
        }
        slog::info(
            "compact",
            slog::context()
                .with_u64("new_segments", 1)
                .with_u64("entries", compacted_entries)
                .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                .with_str("outcome", "success"),
        );
        Ok(1)
    }

    fn compaction_retained_versions(
        &self,
        cf: &ColumnFamily,
        key: &Key,
        versions: &[VersionedValue],
        latest_sequence: SequenceNumber,
        oldest_pinned: Option<SequenceNumber>,
        strategy: crate::config::CompactionStrategy,
    ) -> Vec<crate::segments::block::SegmentEntry> {
        let mut retained = Vec::new();
        let now = now_millis();
        let not_expired = |version: &&VersionedValue| match &version.value {
            StoredValue::PutWithExpiry { expires_at_ms, .. } => *expires_at_ms > now,
            _ => true,
        };

        if versions.is_empty() {
            return retained;
        }

        if matches!(strategy, crate::config::CompactionStrategy::Tiered) {
            for version in versions.iter().filter(not_expired) {
                if let Some(pinned) = oldest_pinned
                    && version.sequence > latest_sequence
                    && version.sequence > pinned
                {
                    continue;
                }
                retained.push(crate::segments::block::SegmentEntry {
                    cf: cf.clone(),
                    key: key.clone(),
                    version: version.clone(),
                });
            }
            return retained;
        }

        if matches!(strategy, crate::config::CompactionStrategy::Universal) {
            for version in versions.iter().rev().filter(not_expired).take(4) {
                retained.push(crate::segments::block::SegmentEntry {
                    cf: cf.clone(),
                    key: key.clone(),
                    version: version.clone(),
                });
            }
            retained.sort_by_key(|entry| entry.version.sequence);
            return retained;
        }

        let Some(newest) = versions
            .iter()
            .rev()
            .filter(not_expired)
            .find(|version| version.sequence <= latest_sequence)
        else {
            return retained;
        };
        if !matches!(newest.value, StoredValue::Delete) || oldest_pinned.is_some() {
            retained.push(crate::segments::block::SegmentEntry {
                cf: cf.clone(),
                key: key.clone(),
                version: newest.clone(),
            });
        }

        let Some(oldest_pinned) = oldest_pinned else {
            return retained;
        };
        let Some(pinned_visible) = versions
            .iter()
            .rev()
            .find(|version| version.sequence <= oldest_pinned)
        else {
            return retained;
        };
        if pinned_visible.sequence != newest.sequence
            && !matches!(pinned_visible.value, StoredValue::Delete)
        {
            retained.push(crate::segments::block::SegmentEntry {
                cf: cf.clone(),
                key: key.clone(),
                version: pinned_visible.clone(),
            });
        }
        retained.sort_by_key(|entry| entry.version.sequence);
        retained
    }

    fn resolve_value_at_snapshot(
        &self,
        cf: &ColumnFamily,
        key: &[u8],
        snapshot: SequenceNumber,
    ) -> Result<Option<Vec<u8>>> {
        let mut chain = self
            .inner
            .segment_index
            .lock()
            .get(&(cf.clone(), Bytes::copy_from_slice(key)))
            .cloned()
            .unwrap_or_default();
        chain.extend(self.inner.memtable.lock().versions_for_key(cf, key));
        chain.sort_by_key(|version| version.sequence);
        self.resolve_versions_chain(cf, &chain, snapshot)
    }

    fn resolve_versions_chain(
        &self,
        cf: &ColumnFamily,
        chain: &[VersionedValue],
        snapshot: SequenceNumber,
    ) -> Result<Option<Vec<u8>>> {
        let merge_operator = self
            .cf_options_map()
            .get(&cf.0)
            .and_then(|options| options.merge_operator.clone())
            .unwrap_or_else(|| "append_bytes".to_string());
        let mut state = None::<Vec<u8>>;
        for version in chain.iter().filter(|version| version.sequence <= snapshot) {
            match version.value {
                StoredValue::Put(ref value) => state = Some(value.clone()),
                StoredValue::PutWithExpiry {
                    ref value,
                    expires_at_ms,
                } => {
                    if expires_at_ms > now_millis() {
                        state = Some(value.clone());
                    } else {
                        state = None;
                    }
                }
                StoredValue::Merge(ref delta) => {
                    let base = state.as_deref().unwrap_or(&[]);
                    state = apply_merge_operator(&merge_operator, base, delta);
                }
                StoredValue::Delete => state = None,
                StoredValue::BlobPointer(ref pointer) => {
                    state = Some(ValueLog::read_pointer(pointer)?);
                }
            }
        }
        Ok(state)
    }

    fn cf_options_map(&self) -> BTreeMap<String, ColumnFamilyOptions> {
        self.inner
            .manifest
            .lock()
            .column_families
            .iter()
            .map(|descriptor| (descriptor.name.clone(), descriptor.options.clone()))
            .collect()
    }

    pub fn checkpoint(&self) -> Result<CheckpointMetadata> {
        let started = std::time::Instant::now();
        self.flush()?;
        let checkpoint_id = self.snapshot().sequence;
        let path = self.inner.layout.checkpoint_path(checkpoint_id);
        let checkpoint = {
            let manifest = self.inner.manifest.lock();
            write_checkpoint(&path, checkpoint_id, &manifest)?
        };
        {
            let mut manifest = self.inner.manifest.lock();
            manifest.latest_checkpoint = Some(checkpoint.clone());
            self.inner.manifest_store.save(&manifest)?;
        }
        self.inner
            .metrics
            .checkpoints
            .fetch_add(1, Ordering::Relaxed);
        slog::info(
            "checkpoint",
            slog::context()
                .with_u64("id", checkpoint.id)
                .with_u64("sequence", checkpoint.sequence)
                .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                .with_str("outcome", "success"),
        );
        Ok(checkpoint)
    }

    pub fn garbage_collect(&self) -> Result<GcReport> {
        let started = std::time::Instant::now();
        let manifest = self.inner.manifest.lock().clone();
        let mut live_files = HashSet::new();
        for segment in &manifest.segments {
            live_files.insert(PathBuf::from(&segment.path));
            live_files.insert(self.inner.layout.value_log_path(segment.id));
        }
        live_files.insert(self.inner.layout.wal_path(manifest.active_wal_id));
        if let Some(checkpoint) = &manifest.latest_checkpoint {
            live_files.insert(PathBuf::from(&checkpoint.path));
        }

        let mut report = GcReport::default();
        self.collect_stale_files(
            &self.inner.layout.segment_root(),
            "tseg",
            &live_files,
            &mut report,
        )?;
        self.collect_stale_files(
            &self.inner.layout.value_root(),
            "tval",
            &live_files,
            &mut report,
        )?;
        self.collect_stale_files(
            &self.inner.layout.wal_root(),
            "wal",
            &live_files,
            &mut report,
        )?;

        self.inner
            .metrics
            .garbage_collections
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .metrics
            .gc_files_reclaimed
            .fetch_add(report.files_reclaimed, Ordering::Relaxed);
        self.inner
            .metrics
            .gc_bytes_reclaimed
            .fetch_add(report.bytes_reclaimed, Ordering::Relaxed);
        slog::info(
            "garbage_collect",
            slog::context()
                .with_u64("files_reclaimed", report.files_reclaimed)
                .with_u64("bytes_reclaimed", report.bytes_reclaimed)
                .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                .with_str("outcome", "success"),
        );
        Ok(report)
    }

    pub fn verify(&self) -> Result<crate::recovery::VerificationReport> {
        let manifest = self.inner.manifest.lock().clone();
        let mut report = crate::recovery::VerificationReport {
            manifest_generation: manifest.next_segment_id,
            segments_checked: 0,
            checkpoints_checked: 0,
            value_logs_checked: 0,
            bytes_checked: 0,
        };
        for segment in &manifest.segments {
            let path = PathBuf::from(&segment.path);
            let metadata = std::fs::metadata(&path)?;
            let digest = crate::io::file_digest(&path)?.to_hex().to_string();
            if digest != segment.file_digest {
                return Err(TridentError::Corrupt {
                    path,
                    reason: "segment digest mismatch".to_string(),
                });
            }
            report.segments_checked += 1;
            report.bytes_checked += metadata.len();
            let value_log_path = self.inner.layout.value_log_path(segment.id);
            if value_log_path.exists() {
                report.value_logs_checked += 1;
                report.bytes_checked += std::fs::metadata(value_log_path)?.len();
            }
        }
        if let Some(checkpoint) = &manifest.latest_checkpoint {
            let path = PathBuf::from(&checkpoint.path);
            let metadata = std::fs::metadata(&path)?;
            let digest = crate::io::file_digest(&path)?.to_hex().to_string();
            if digest != checkpoint.file_digest {
                return Err(TridentError::Corrupt {
                    path,
                    reason: "checkpoint digest mismatch".to_string(),
                });
            }
            report.checkpoints_checked += 1;
            report.bytes_checked += metadata.len();
        }
        Ok(report)
    }

    pub fn backup_to(&self, backup_dir: impl AsRef<Path>) -> Result<()> {
        let started = std::time::Instant::now();
        self.checkpoint()?;
        let backup_dir = backup_dir.as_ref();
        if backup_dir.exists() {
            fs::remove_dir_all(backup_dir)?;
        }
        fs::create_dir_all(backup_dir)?;
        copy_dir_recursive(self.inner.layout.root(), backup_dir)?;
        slog::info(
            "backup_complete",
            slog::context()
                .with_str("backup_dir", backup_dir.to_string_lossy().to_string())
                .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                .with_str("outcome", "success"),
        );
        Ok(())
    }

    pub fn restore_from_backup(
        backup_dir: impl AsRef<Path>,
        target_dir: impl AsRef<Path>,
    ) -> Result<()> {
        let backup_dir = backup_dir.as_ref();
        let target_dir = target_dir.as_ref();
        if !backup_dir.exists() {
            return Err(TridentError::InvalidConfig(format!(
                "backup directory does not exist: {}",
                backup_dir.to_string_lossy()
            )));
        }
        if target_dir.exists() {
            fs::remove_dir_all(target_dir)?;
        }
        fs::create_dir_all(target_dir)?;
        copy_dir_recursive(backup_dir, target_dir)?;
        Ok(())
    }

    pub fn close(&self) -> Result<()> {
        self.flush().map(|_| ())
    }

    fn collect_stale_files(
        &self,
        root: &Path,
        extension: &str,
        live_files: &HashSet<PathBuf>,
        report: &mut GcReport,
    ) -> Result<()> {
        if !root.exists() {
            return Ok(());
        }
        let mut stack = vec![root.to_path_buf()];
        while let Some(path) = stack.pop() {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some(extension) {
                    continue;
                }
                if live_files.contains(&path) {
                    continue;
                }
                let len = entry.metadata()?.len();
                std::fs::remove_file(&path)?;
                report.files_reclaimed += 1;
                report.bytes_reclaimed += len;
            }
        }
        Ok(())
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
            "effective_config": self.effective_config(),
            "metrics": self.inner.metrics.snapshot(),
            "manifest": &*self.inner.manifest.lock(),
            "active_wal": self.active_wal_stats(),
            "snapshots": {
                "pinned_count": self.inner.snapshots.pinned_count(),
                "oldest_pinned_sequence": self.inner.snapshots.oldest_pinned_sequence(),
            },
            "data_dir": self.inner.layout.root(),
        })
    }

    fn active_wal_stats(&self) -> serde_json::Value {
        let wal = self.inner.wal.lock();
        serde_json::json!({
            "id": wal.segment_id(),
            "path": wal.path().to_string_lossy(),
        })
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn apply_merge_operator(name: &str, current: &[u8], delta: &[u8]) -> Option<Vec<u8>> {
    match name {
        "sum_i64" => {
            let left = if current.is_empty() {
                0_i64
            } else {
                i64::from_le_bytes(current.try_into().ok()?)
            };
            let right = i64::from_le_bytes(delta.try_into().ok()?);
            Some((left + right).to_le_bytes().to_vec())
        }
        "max_u64" => {
            let left = if current.is_empty() {
                0_u64
            } else {
                u64::from_le_bytes(current.try_into().ok()?)
            };
            let right = u64::from_le_bytes(delta.try_into().ok()?);
            Some(left.max(right).to_le_bytes().to_vec())
        }
        _ => {
            let mut merged = Vec::with_capacity(current.len() + delta.len());
            merged.extend_from_slice(current);
            merged.extend_from_slice(delta);
            Some(merged)
        }
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let src = entry.path();
        let dst = destination.join(entry.file_name());
        if src.is_dir() {
            fs::create_dir_all(&dst)?;
            copy_dir_recursive(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
    if prefix.is_empty() {
        return None;
    }
    let mut out = prefix.to_vec();
    for index in (0..out.len()).rev() {
        if out[index] != u8::MAX {
            out[index] += 1;
            out.truncate(index + 1);
            return Some(out);
        }
    }
    None
}

impl Clone for TridentEngine {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl crate::disk::PoiesisStorageAdapter for TridentEngine {
    fn put_page_value(&self, key: Key, value: Value) -> Result<SequenceNumber> {
        self.put(key, value)
    }

    fn read_page_value(&self, key: &[u8], snapshot: ReadSnapshot) -> Result<Option<Value>> {
        self.get_cf(&ColumnFamily::default(), key, snapshot)
    }

    fn write_kv_batch(&self, batch: WriteBatch) -> Result<SequenceNumber> {
        self.write_batch(batch)
    }

    fn flush_durable(&self) -> Result<()> {
        self.flush().map(|_| ())
    }
}
