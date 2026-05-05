use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub wal_records_replayed: u64,
    pub last_sequence: u64,
    pub segment_count: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub manifest_generation: u64,
    pub segments_checked: u64,
    pub checkpoints_checked: u64,
    pub value_logs_checked: u64,
    pub bytes_checked: u64,
}
