use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct EngineMetrics {
    pub writes: AtomicU64,
    pub reads: AtomicU64,
    pub deletes: AtomicU64,
    pub wal_records: AtomicU64,
    pub segment_flushes: AtomicU64,
    pub automatic_flushes: AtomicU64,
    pub memtable_bytes: AtomicU64,
    pub write_stalls: AtomicU64,
    pub l0_pressure_compactions: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub bloom_negative_hits: AtomicU64,
    pub recovered_records: AtomicU64,
    pub checkpoints: AtomicU64,
    pub garbage_collections: AtomicU64,
    pub gc_files_reclaimed: AtomicU64,
    pub gc_bytes_reclaimed: AtomicU64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineMetricsSnapshot {
    pub writes: u64,
    pub reads: u64,
    pub deletes: u64,
    pub wal_records: u64,
    pub segment_flushes: u64,
    pub automatic_flushes: u64,
    pub memtable_bytes: u64,
    pub write_stalls: u64,
    pub l0_pressure_compactions: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub bloom_negative_hits: u64,
    pub recovered_records: u64,
    pub checkpoints: u64,
    pub garbage_collections: u64,
    pub gc_files_reclaimed: u64,
    pub gc_bytes_reclaimed: u64,
}

impl EngineMetrics {
    pub fn snapshot(&self) -> EngineMetricsSnapshot {
        EngineMetricsSnapshot {
            writes: self.writes.load(Ordering::Relaxed),
            reads: self.reads.load(Ordering::Relaxed),
            deletes: self.deletes.load(Ordering::Relaxed),
            wal_records: self.wal_records.load(Ordering::Relaxed),
            segment_flushes: self.segment_flushes.load(Ordering::Relaxed),
            automatic_flushes: self.automatic_flushes.load(Ordering::Relaxed),
            memtable_bytes: self.memtable_bytes.load(Ordering::Relaxed),
            write_stalls: self.write_stalls.load(Ordering::Relaxed),
            l0_pressure_compactions: self.l0_pressure_compactions.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            bloom_negative_hits: self.bloom_negative_hits.load(Ordering::Relaxed),
            recovered_records: self.recovered_records.load(Ordering::Relaxed),
            checkpoints: self.checkpoints.load(Ordering::Relaxed),
            garbage_collections: self.garbage_collections.load(Ordering::Relaxed),
            gc_files_reclaimed: self.gc_files_reclaimed.load(Ordering::Relaxed),
            gc_bytes_reclaimed: self.gc_bytes_reclaimed.load(Ordering::Relaxed),
        }
    }
}
