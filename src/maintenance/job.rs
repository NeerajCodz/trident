use crate::config::CompactionStrategy;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum JobPriority {
    High = 0,
    Normal = 1,
    Low = 2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaintenanceJob {
    Flush {
        reason: String,
    },
    Compact {
        strategy: CompactionStrategy,
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedJob {
    pub id: u64,
    pub priority: JobPriority,
    pub job: MaintenanceJob,
}
