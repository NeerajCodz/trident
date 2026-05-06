use crate::accel::Accelerator;
use crate::cache::BlockCache;
use crate::config::TridentConfig;
use crate::disk::DiskLayout;
use crate::maintenance::{MaintenanceRuntimeController, MaintenanceScheduler};
use crate::manifest::Manifest;
use crate::manifest::ManifestStore;
use crate::metrics::EngineMetrics;
use crate::memory::{MemTable, SnapshotManager};
use crate::types::{ColumnFamily, Key, VersionedValue};
use crate::wal::Wal;
use parking_lot::{Condvar, Mutex};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) type CacheKey = (String, u64, Vec<u8>);
pub(crate) type SegmentIndex = BTreeMap<(ColumnFamily, Key), Vec<VersionedValue>>;

#[derive(Default)]
pub(crate) struct GroupCommitState {
    pub(crate) enqueued_batch: u64,
    pub(crate) synced_batch: u64,
    pub(crate) leader_active: bool,
}

#[derive(Default)]
pub(crate) struct GroupCommitCoordinator {
    pub(crate) state: Mutex<GroupCommitState>,
    pub(crate) ready: Condvar,
}

pub(crate) struct EngineInner {
    pub(crate) config: TridentConfig,
    pub(crate) layout: DiskLayout,
    pub(crate) manifest_store: ManifestStore,
    pub(crate) cas_serialization: Mutex<()>,
    pub(crate) manifest: Mutex<Manifest>,
    pub(crate) wal: Mutex<Wal>,
    pub(crate) memtable: Mutex<MemTable>,
    pub(crate) segment_index: Mutex<SegmentIndex>,
    pub(crate) cache: Mutex<BlockCache<CacheKey>>,
    pub(crate) cache_bytes_by_cf: Mutex<BTreeMap<String, usize>>,
    pub(crate) scheduler: Mutex<MaintenanceScheduler>,
    pub(crate) runtime: Mutex<MaintenanceRuntimeController>,
    pub(crate) group_commit: GroupCommitCoordinator,
    pub(crate) snapshots: Arc<SnapshotManager>,
    pub(crate) metrics: EngineMetrics,
    pub(crate) accelerator: Arc<dyn Accelerator>,
}

