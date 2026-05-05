use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub wal_records_replayed: u64,
    pub last_sequence: u64,
    pub segment_count: u64,
}
