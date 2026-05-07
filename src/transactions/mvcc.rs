use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct VersionStamp {
    pub create_sequence: u64,
    pub delete_sequence: Option<u64>,
    pub commit_timestamp_ms: Option<u64>,
    pub expires_at_ms: Option<u64>,
}

impl VersionStamp {
    pub fn visible_at(self, snapshot: VisibilityWatermark) -> bool {
        self.create_sequence <= snapshot.sequence
            && self
                .delete_sequence
                .is_none_or(|delete_sequence| delete_sequence > snapshot.sequence)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct VisibilityWatermark {
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TransactionVisibility {
    pub watermark: VisibilityWatermark,
    pub active_transactions: BTreeSet<u64>,
    pub gc_horizon: GcHorizon,
}

impl TransactionVisibility {
    pub fn can_see(&self, version: VersionStamp) -> bool {
        version.visible_at(self.watermark)
            && !self.active_transactions.contains(&version.create_sequence)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct GcHorizon {
    pub sequence: u64,
}

impl GcHorizon {
    pub fn can_collect(self, version: VersionStamp) -> bool {
        version
            .delete_sequence
            .is_some_and(|delete_sequence| delete_sequence <= self.sequence)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SnapshotRegistry {
    active: BTreeSet<u64>,
}

impl SnapshotRegistry {
    pub fn pin(&mut self, watermark: VisibilityWatermark) {
        self.active.insert(watermark.sequence);
    }

    pub fn unpin(&mut self, watermark: VisibilityWatermark) {
        self.active.remove(&watermark.sequence);
    }

    pub fn gc_horizon(&self, latest: u64) -> GcHorizon {
        GcHorizon {
            sequence: self.active.first().copied().unwrap_or(latest),
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }
}

#[derive(Debug)]
pub struct EpochGuard<'a> {
    registry: &'a AtomicU64,
}

impl<'a> EpochGuard<'a> {
    pub fn enter(registry: &'a AtomicU64) -> Self {
        registry.fetch_add(1, Ordering::SeqCst);
        Self { registry }
    }
}

impl Drop for EpochGuard<'_> {
    fn drop(&mut self) {
        self.registry.fetch_sub(1, Ordering::SeqCst);
    }
}
