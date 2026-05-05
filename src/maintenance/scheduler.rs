use crate::maintenance::job::{JobPriority, QueuedJob};
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct MaintenanceScheduler {
    next_id: u64,
    high: VecDeque<QueuedJob>,
    normal: VecDeque<QueuedJob>,
    low: VecDeque<QueuedJob>,
}

impl MaintenanceScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&mut self, mut job: QueuedJob) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        job.id = self.next_id;
        match job.priority {
            JobPriority::High => self.high.push_back(job),
            JobPriority::Normal => self.normal.push_back(job),
            JobPriority::Low => self.low.push_back(job),
        }
        self.next_id
    }

    pub fn dequeue(&mut self) -> Option<QueuedJob> {
        self.high
            .pop_front()
            .or_else(|| self.normal.pop_front())
            .or_else(|| self.low.pop_front())
    }

    pub fn len(&self) -> usize {
        self.high.len() + self.normal.len() + self.low.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
