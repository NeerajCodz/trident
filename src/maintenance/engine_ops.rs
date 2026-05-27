use crate::config::CompactionStrategy;
use crate::engine::core::engine::PraxisEngine;
use crate::errors::{PraxisError, Result};
use crate::maintenance::{
    JobFailureDisposition, JobPriority, MaintenanceJob, MaintenanceLane, MaintenanceRuntimeConfig,
    MaintenanceStatusSnapshot, QueuedJob,
};
use crate::slog;

impl PraxisEngine {
    pub fn enqueue_flush_job(
        &self,
        reason: impl Into<String>,
        priority: JobPriority,
    ) -> Result<u64> {
        self.enqueue_job(
            MaintenanceLane::Flush,
            MaintenanceJob::Flush {
                reason: reason.into(),
            },
            priority,
        )
    }

    pub fn enqueue_admin_flush_job(
        &self,
        reason: impl Into<String>,
        priority: JobPriority,
    ) -> Result<u64> {
        self.enqueue_job(
            MaintenanceLane::Admin,
            MaintenanceJob::Flush {
                reason: reason.into(),
            },
            priority,
        )
    }

    pub fn enqueue_compaction_job(
        &self,
        strategy: CompactionStrategy,
        reason: impl Into<String>,
        priority: JobPriority,
    ) -> Result<u64> {
        self.enqueue_job(
            MaintenanceLane::Compaction,
            MaintenanceJob::Compact {
                strategy,
                reason: reason.into(),
            },
            priority,
        )
    }

    pub fn enqueue_admin_compaction_job(
        &self,
        strategy: CompactionStrategy,
        reason: impl Into<String>,
        priority: JobPriority,
    ) -> Result<u64> {
        self.enqueue_job(
            MaintenanceLane::Admin,
            MaintenanceJob::Compact {
                strategy,
                reason: reason.into(),
            },
            priority,
        )
    }

    pub fn run_next_maintenance_job(&self) -> Result<Option<u64>> {
        let Some(job) = self.inner.scheduler.lock().dequeue_next() else {
            return Ok(None);
        };
        self.execute_queued_job(job)
    }

    pub fn run_next_maintenance_job_for_lane(&self, lane: MaintenanceLane) -> Result<Option<u64>> {
        let Some(job) = self.inner.scheduler.lock().dequeue_for_lane(lane) else {
            return Ok(None);
        };
        self.execute_queued_job(job)
    }

    pub fn run_maintenance_workers(
        &self,
        worker_count: usize,
        max_jobs_per_worker: usize,
    ) -> Result<usize> {
        let worker_count = worker_count.max(1);
        let max_jobs_per_worker = max_jobs_per_worker.max(1);
        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let engine = self.clone();
            handles.push(std::thread::spawn(move || -> Result<usize> {
                let mut completed = 0usize;
                for _ in 0..max_jobs_per_worker {
                    if engine.run_next_maintenance_job()?.is_none() {
                        break;
                    }
                    completed += 1;
                }
                Ok(completed)
            }));
        }
        let mut total = 0usize;
        for handle in handles {
            let completed = handle
                .join()
                .map_err(|_| PraxisError::TaskJoin("maintenance worker panicked".to_string()))??;
            total += completed;
        }
        Ok(total)
    }

    pub fn start_maintenance_runtime(&self, config: MaintenanceRuntimeConfig) -> Result<()> {
        self.inner.runtime.lock().start(self.clone(), config)
    }

    pub fn stop_maintenance_runtime(&self) -> Result<()> {
        self.inner.runtime.lock().stop()
    }

    pub fn join_maintenance_runtime(&self) -> Result<()> {
        self.inner.runtime.lock().join()
    }

    pub fn maintenance_status(&self) -> MaintenanceStatusSnapshot {
        let runtime = self.inner.runtime.lock().status();
        self.inner.scheduler.lock().snapshot(runtime)
    }

    pub fn retry_maintenance_job(&self, job_id: u64) -> Result<u64> {
        let mut scheduler = self.inner.scheduler.lock();
        let retried = scheduler.retry_failed(job_id).ok_or_else(|| {
            if scheduler.has_failed_job(job_id) {
                PraxisError::WriteStalled {
                    reason: "maintenance queue capacity reached".to_string(),
                }
            } else {
                PraxisError::MaintenanceJobNotFound(job_id)
            }
        })?;
        slog::info(
            "maintenance_job_retried",
            slog::context()
                .with_u64("failed_job_id", job_id)
                .with_u64("new_job_id", retried),
        );
        Ok(retried)
    }

    fn enqueue_job(
        &self,
        lane: MaintenanceLane,
        job: MaintenanceJob,
        priority: JobPriority,
    ) -> Result<u64> {
        let id = self
            .inner
            .scheduler
            .lock()
            .enqueue(QueuedJob {
                id: 0,
                priority,
                lane,
                attempts: 0,
                queued_at_ms: now_millis(),
                source_failed_job_id: None,
                job,
            })
            .ok_or_else(|| PraxisError::WriteStalled {
                reason: "maintenance queue capacity reached".to_string(),
            })?;
        slog::info(
            "maintenance_job_queued",
            slog::context()
                .with_u64("job_id", id)
                .with_str("lane", format!("{lane:?}"))
                .with_str("priority", format!("{priority:?}")),
        );
        Ok(id)
    }

    fn execute_queued_job(&self, job: QueuedJob) -> Result<Option<u64>> {
        let started = std::time::Instant::now();
        slog::info(
            "maintenance_job_started",
            slog::context()
                .with_u64("job_id", job.id)
                .with_str("lane", format!("{:?}", job.lane))
                .with_str("priority", format!("{:?}", job.priority))
                .with_u64("attempts", job.attempts as u64),
        );
        let (job_type, reason, result) = match &job.job {
            MaintenanceJob::Flush { reason } => ("flush", reason.clone(), self.flush().map(|_| ())),
            MaintenanceJob::Compact { strategy, reason } => (
                "compact",
                reason.clone(),
                self.compact_with_strategy(*strategy).map(|_| ()),
            ),
        };
        match result {
            Ok(()) => {
                self.inner.scheduler.lock().complete(job.id);
                slog::info(
                    "maintenance_job_complete",
                    slog::context()
                        .with_u64("job_id", job.id)
                        .with_str("job_type", job_type)
                        .with_str("reason", reason)
                        .with_str("lane", format!("{:?}", job.lane))
                        .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                        .with_str("outcome", "success"),
                );
                Ok(Some(job.id))
            }
            Err(error) => {
                let retryable = error.is_retryable();
                let message = error.to_string();
                let disposition =
                    self.inner
                        .scheduler
                        .lock()
                        .fail(job.clone(), message.clone(), retryable);
                let event = match disposition {
                    JobFailureDisposition::Requeued => "maintenance_job_requeued",
                    JobFailureDisposition::Terminal => "maintenance_job_terminal_failed",
                };
                slog::info(
                    event,
                    slog::context()
                        .with_u64("job_id", job.id)
                        .with_str("job_type", job_type)
                        .with_str("reason", reason)
                        .with_str("lane", format!("{:?}", job.lane))
                        .with_bool("retryable", retryable)
                        .with_u64("attempts", job.attempts as u64 + 1)
                        .with_u64("duration_ms", started.elapsed().as_millis() as u64)
                        .with_str("error", message),
                );
                Err(error)
            }
        }
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
