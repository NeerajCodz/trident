use crate::maintenance::job::{
    FailedJobRecord, JobPriority, MaintenanceLane, MaintenanceStatusSnapshot, QueuedJob,
    RunningJobRecord, RuntimeStatusSnapshot,
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobFailureDisposition {
    Requeued,
    Terminal,
}

#[derive(Debug, Default)]
struct LaneQueue {
    rr_cursor: u8,
    high: VecDeque<QueuedJob>,
    normal: VecDeque<QueuedJob>,
    low: VecDeque<QueuedJob>,
}

impl LaneQueue {
    fn enqueue(&mut self, job: QueuedJob) {
        match job.priority {
            JobPriority::High => self.high.push_back(job),
            JobPriority::Normal => self.normal.push_back(job),
            JobPriority::Low => self.low.push_back(job),
        }
    }

    fn requeue_front(&mut self, job: QueuedJob) {
        match job.priority {
            JobPriority::High => self.high.push_front(job),
            JobPriority::Normal => self.normal.push_front(job),
            JobPriority::Low => self.low.push_front(job),
        }
    }

    fn dequeue(&mut self) -> Option<QueuedJob> {
        for _ in 0..4 {
            self.rr_cursor = (self.rr_cursor + 1) % 4;
            let candidate = match self.rr_cursor {
                0 | 1 => self.high.pop_front(),
                2 => self.normal.pop_front(),
                _ => self.low.pop_front(),
            };
            if candidate.is_some() {
                return candidate;
            }
        }
        self.high
            .pop_front()
            .or_else(|| self.normal.pop_front())
            .or_else(|| self.low.pop_front())
    }

    fn len(&self) -> usize {
        self.high.len() + self.normal.len() + self.low.len()
    }

    fn highest_priority(&self) -> Option<JobPriority> {
        if !self.high.is_empty() {
            Some(JobPriority::High)
        } else if !self.normal.is_empty() {
            Some(JobPriority::Normal)
        } else if !self.low.is_empty() {
            Some(JobPriority::Low)
        } else {
            None
        }
    }

    fn queued_jobs(&self) -> Vec<QueuedJob> {
        self.high
            .iter()
            .chain(self.normal.iter())
            .chain(self.low.iter())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct MaintenanceScheduler {
    next_id: u64,
    capacity: usize,
    retry_limit: u8,
    flush: LaneQueue,
    compaction: LaneQueue,
    admin: LaneQueue,
    running: BTreeMap<u64, RunningJobRecord>,
    failed: BTreeMap<u64, FailedJobRecord>,
}

impl MaintenanceScheduler {
    pub fn new(capacity: usize, retry_limit: u8) -> Self {
        Self {
            capacity,
            retry_limit,
            ..Self::default()
        }
    }

    pub fn enqueue(&mut self, mut job: QueuedJob) -> Option<u64> {
        if self.len() >= self.capacity {
            return None;
        }
        self.next_id = self.next_id.saturating_add(1);
        job.id = self.next_id;
        self.queue_for_lane(job.lane).enqueue(job);
        Some(self.next_id)
    }

    pub fn dequeue_for_lane(&mut self, lane: MaintenanceLane) -> Option<QueuedJob> {
        let job = self.queue_for_lane(lane).dequeue()?;
        self.running.insert(
            job.id,
            RunningJobRecord {
                id: job.id,
                priority: job.priority,
                lane: job.lane,
                attempts: job.attempts,
                queued_at_ms: job.queued_at_ms,
                started_at_ms: now_millis(),
                source_failed_job_id: job.source_failed_job_id,
                job: job.job.clone(),
            },
        );
        Some(job)
    }

    pub fn dequeue_next(&mut self) -> Option<QueuedJob> {
        let mut candidates = [
            (MaintenanceLane::Admin, self.admin.highest_priority()),
            (MaintenanceLane::Flush, self.flush.highest_priority()),
            (
                MaintenanceLane::Compaction,
                self.compaction.highest_priority(),
            ),
        ]
        .into_iter()
        .filter_map(|(lane, priority)| priority.map(|priority| (lane, priority)))
        .collect::<Vec<_>>();
        candidates.sort_by_key(|(lane, priority)| (*priority, *lane));
        candidates
            .into_iter()
            .find_map(|(lane, _)| self.dequeue_for_lane(lane))
    }

    pub fn complete(&mut self, job_id: u64) {
        self.running.remove(&job_id);
    }

    pub fn fail(
        &mut self,
        mut job: QueuedJob,
        last_error: String,
        retryable: bool,
    ) -> JobFailureDisposition {
        let started_at_ms = self
            .running
            .remove(&job.id)
            .map(|running| running.started_at_ms);
        job.attempts = job.attempts.saturating_add(1);
        if retryable && job.attempts < self.retry_limit {
            self.queue_for_lane(job.lane).requeue_front(job);
            return JobFailureDisposition::Requeued;
        }
        self.failed.insert(
            job.id,
            FailedJobRecord {
                id: job.id,
                priority: job.priority,
                lane: job.lane,
                attempts: job.attempts,
                queued_at_ms: job.queued_at_ms,
                started_at_ms,
                finished_at_ms: now_millis(),
                retryable,
                last_error,
                source_failed_job_id: job.source_failed_job_id,
                job: job.job,
            },
        );
        JobFailureDisposition::Terminal
    }

    pub fn retry_failed(&mut self, failed_job_id: u64) -> Option<u64> {
        if self.len() >= self.capacity {
            return None;
        }
        let failed = self.failed.remove(&failed_job_id)?;
        self.enqueue(QueuedJob {
            id: 0,
            priority: failed.priority,
            lane: failed.lane,
            attempts: 0,
            queued_at_ms: now_millis(),
            source_failed_job_id: Some(failed.id),
            job: failed.job,
        })
    }

    pub fn len(&self) -> usize {
        self.flush.len() + self.compaction.len() + self.admin.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn failed_len(&self) -> usize {
        self.failed.len()
    }

    pub fn snapshot(&self, runtime: RuntimeStatusSnapshot) -> MaintenanceStatusSnapshot {
        let mut queued = self.flush.queued_jobs();
        queued.extend(self.compaction.queued_jobs());
        queued.extend(self.admin.queued_jobs());
        queued.sort_by_key(|job| (job.lane, job.priority, job.id));

        let mut running = self.running.values().cloned().collect::<Vec<_>>();
        running.sort_by_key(|job| (job.lane, job.priority, job.id));

        let mut failed = self.failed.values().cloned().collect::<Vec<_>>();
        failed.sort_by_key(|job| (job.finished_at_ms, job.id));

        MaintenanceStatusSnapshot {
            capacity: self.capacity,
            retry_limit: self.retry_limit,
            queued,
            running,
            failed,
            runtime,
        }
    }

    pub fn has_failed_job(&self, job_id: u64) -> bool {
        self.failed.contains_key(&job_id)
    }

    fn queue_for_lane(&mut self, lane: MaintenanceLane) -> &mut LaneQueue {
        match lane {
            MaintenanceLane::Flush => &mut self.flush,
            MaintenanceLane::Compaction => &mut self.compaction,
            MaintenanceLane::Admin => &mut self.admin,
        }
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CompactionStrategy;
    use crate::maintenance::MaintenanceJob;

    fn queued_compaction_job() -> QueuedJob {
        QueuedJob {
            id: 0,
            priority: JobPriority::Normal,
            lane: MaintenanceLane::Compaction,
            attempts: 0,
            queued_at_ms: 10,
            source_failed_job_id: None,
            job: MaintenanceJob::Compact {
                strategy: CompactionStrategy::Leveled,
                reason: "test".to_string(),
            },
        }
    }

    #[test]
    fn retryable_failure_requeues_until_retry_limit() {
        let mut scheduler = MaintenanceScheduler::new(16, 3);
        let id = scheduler.enqueue(queued_compaction_job()).unwrap();
        let job = scheduler
            .dequeue_for_lane(MaintenanceLane::Compaction)
            .unwrap();
        assert_eq!(job.id, id);

        let disposition = scheduler.fail(job, "temporary".to_string(), true);
        assert_eq!(disposition, JobFailureDisposition::Requeued);
        assert_eq!(scheduler.len(), 1);
        assert_eq!(scheduler.failed_len(), 0);

        let retried = scheduler
            .dequeue_for_lane(MaintenanceLane::Compaction)
            .unwrap();
        assert_eq!(retried.attempts, 1);
    }

    #[test]
    fn retryable_failure_becomes_terminal_at_limit() {
        let mut scheduler = MaintenanceScheduler::new(16, 2);
        let id = scheduler.enqueue(queued_compaction_job()).unwrap();

        let job = scheduler
            .dequeue_for_lane(MaintenanceLane::Compaction)
            .unwrap();
        assert_eq!(
            scheduler.fail(job, "temporary-1".to_string(), true),
            JobFailureDisposition::Requeued
        );

        let job = scheduler
            .dequeue_for_lane(MaintenanceLane::Compaction)
            .unwrap();
        assert_eq!(job.id, id);
        assert_eq!(job.attempts, 1);
        assert_eq!(
            scheduler.fail(job, "temporary-2".to_string(), true),
            JobFailureDisposition::Terminal
        );
        assert_eq!(scheduler.len(), 0);
        assert_eq!(scheduler.failed_len(), 1);

        let failed = scheduler
            .snapshot(RuntimeStatusSnapshot::default())
            .failed
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(failed.id, id);
        assert_eq!(failed.attempts, 2);
        assert!(failed.retryable);
        assert_eq!(failed.last_error, "temporary-2");
    }

    #[test]
    fn manual_retry_creates_new_job_linked_to_failed_record() {
        let mut scheduler = MaintenanceScheduler::new(16, 1);
        let id = scheduler.enqueue(queued_compaction_job()).unwrap();
        let job = scheduler
            .dequeue_for_lane(MaintenanceLane::Compaction)
            .unwrap();
        assert_eq!(
            scheduler.fail(job, "permanent".to_string(), true),
            JobFailureDisposition::Terminal
        );

        let retried_id = scheduler.retry_failed(id).unwrap();
        assert_ne!(retried_id, id);
        let queued = scheduler
            .snapshot(RuntimeStatusSnapshot::default())
            .queued
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(queued.id, retried_id);
        assert_eq!(queued.attempts, 0);
        assert_eq!(queued.source_failed_job_id, Some(id));
    }
}
