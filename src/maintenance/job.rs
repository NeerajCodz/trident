use crate::config::CompactionStrategy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum JobPriority {
    High = 0,
    Normal = 1,
    Low = 2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MaintenanceJob {
    Flush {
        reason: String,
    },
    Compact {
        strategy: CompactionStrategy,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum MaintenanceLane {
    Flush,
    Compaction,
    Admin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueuedJob {
    pub id: u64,
    pub priority: JobPriority,
    pub lane: MaintenanceLane,
    pub attempts: u8,
    pub queued_at_ms: u64,
    pub source_failed_job_id: Option<u64>,
    pub job: MaintenanceJob,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunningJobRecord {
    pub id: u64,
    pub priority: JobPriority,
    pub lane: MaintenanceLane,
    pub attempts: u8,
    pub queued_at_ms: u64,
    pub started_at_ms: u64,
    pub source_failed_job_id: Option<u64>,
    pub job: MaintenanceJob,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FailedJobRecord {
    pub id: u64,
    pub priority: JobPriority,
    pub lane: MaintenanceLane,
    pub attempts: u8,
    pub queued_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: u64,
    pub retryable: bool,
    pub last_error: String,
    pub source_failed_job_id: Option<u64>,
    pub job: MaintenanceJob,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeLaneConfig {
    pub workers: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceRuntimeConfig {
    pub flush: RuntimeLaneConfig,
    pub compaction: RuntimeLaneConfig,
    pub admin: RuntimeLaneConfig,
    pub idle_sleep_ms: u64,
}

impl Default for MaintenanceRuntimeConfig {
    fn default() -> Self {
        Self {
            flush: RuntimeLaneConfig { workers: 1 },
            compaction: RuntimeLaneConfig { workers: 1 },
            admin: RuntimeLaneConfig { workers: 1 },
            idle_sleep_ms: 25,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeStatusSnapshot {
    pub running: bool,
    pub stop_requested: bool,
    pub started_at_ms: Option<u64>,
    pub workers_by_lane: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceStatusSnapshot {
    pub capacity: usize,
    pub retry_limit: u8,
    pub queued: Vec<QueuedJob>,
    pub running: Vec<RunningJobRecord>,
    pub failed: Vec<FailedJobRecord>,
    pub runtime: RuntimeStatusSnapshot,
}
