use crate::types::{ReadSnapshot, SequenceNumber};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct SnapshotManager {
    latest_sequence: AtomicU64,
    next_pin_id: AtomicU64,
    pinned: Mutex<BTreeMap<u64, SequenceNumber>>,
}

#[derive(Debug)]
pub struct PinnedSnapshot {
    id: u64,
    snapshot: ReadSnapshot,
    manager: Arc<SnapshotManager>,
}

impl SnapshotManager {
    pub fn next_sequence(&self) -> SequenceNumber {
        self.latest_sequence.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn observe(&self, sequence: SequenceNumber) {
        let mut current = self.latest_sequence.load(Ordering::SeqCst);
        while sequence > current {
            match self.latest_sequence.compare_exchange(
                current,
                sequence,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        ReadSnapshot {
            sequence: self.latest_sequence.load(Ordering::SeqCst),
        }
    }

    pub fn pin(self: &Arc<Self>) -> PinnedSnapshot {
        let snapshot = self.snapshot();
        let id = self.next_pin_id.fetch_add(1, Ordering::SeqCst) + 1;
        self.pinned.lock().insert(id, snapshot.sequence);
        PinnedSnapshot {
            id,
            snapshot,
            manager: self.clone(),
        }
    }

    pub fn oldest_pinned_sequence(&self) -> Option<SequenceNumber> {
        self.pinned.lock().values().copied().min()
    }

    pub fn pinned_count(&self) -> usize {
        self.pinned.lock().len()
    }

    fn unpin(&self, id: u64) {
        self.pinned.lock().remove(&id);
    }
}

impl PinnedSnapshot {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        self.snapshot
    }

    pub fn sequence(&self) -> SequenceNumber {
        self.snapshot.sequence
    }
}

impl Drop for PinnedSnapshot {
    fn drop(&mut self) {
        self.manager.unpin(self.id);
    }
}
