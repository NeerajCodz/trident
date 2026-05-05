use crate::types::{ReadSnapshot, SequenceNumber};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct SnapshotManager {
    latest_sequence: AtomicU64,
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
}
