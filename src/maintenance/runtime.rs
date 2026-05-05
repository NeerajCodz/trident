use crate::engine::TridentEngine;
use crate::errors::{Result, TridentError};
use crate::maintenance::job::{MaintenanceLane, MaintenanceRuntimeConfig, RuntimeStatusSnapshot};
use crate::slog;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug)]
struct RuntimeWorkerHandle {
    lane: MaintenanceLane,
    handle: JoinHandle<()>,
}

#[derive(Debug, Default)]
pub struct MaintenanceRuntimeController {
    stop: Option<Arc<AtomicBool>>,
    started_at_ms: Option<u64>,
    config: Option<MaintenanceRuntimeConfig>,
    workers: Vec<RuntimeWorkerHandle>,
}

impl MaintenanceRuntimeController {
    pub fn start(&mut self, engine: TridentEngine, config: MaintenanceRuntimeConfig) -> Result<()> {
        if self.is_running() {
            return Err(TridentError::MaintenanceRuntimeRunning);
        }
        let total_workers = config.flush.workers + config.compaction.workers + config.admin.workers;
        if total_workers == 0 {
            return Err(TridentError::InvalidConfig(
                "maintenance runtime requires at least one worker".to_string(),
            ));
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.started_at_ms = Some(now_millis());
        self.config = Some(config.clone());
        for (lane, worker_count) in [
            (MaintenanceLane::Flush, config.flush.workers),
            (MaintenanceLane::Compaction, config.compaction.workers),
            (MaintenanceLane::Admin, config.admin.workers),
        ] {
            for worker_index in 0..worker_count {
                let stop_signal = stop.clone();
                let worker_engine = engine.clone();
                let idle_sleep_ms = config.idle_sleep_ms.max(1);
                let name = format!("trident-{lane:?}-{worker_index}");
                let handle = thread::Builder::new()
                    .name(name)
                    .spawn(move || {
                        while !stop_signal.load(Ordering::Relaxed) {
                            match worker_engine.run_next_maintenance_job_for_lane(lane) {
                                Ok(Some(_)) => {}
                                Ok(None) => thread::sleep(Duration::from_millis(idle_sleep_ms)),
                                Err(error) => {
                                    slog::error(
                                        "maintenance_runtime_worker_error",
                                        slog::context()
                                            .with_str("lane", format!("{lane:?}"))
                                            .with_str("error", error.to_string())
                                            .with_bool("retryable", error.is_retryable()),
                                    );
                                    thread::sleep(Duration::from_millis(idle_sleep_ms));
                                }
                            }
                        }
                    })
                    .map_err(TridentError::Io)?;
                self.workers.push(RuntimeWorkerHandle { lane, handle });
            }
        }
        self.stop = Some(stop);
        slog::info(
            "maintenance_runtime_started",
            slog::context()
                .with_u64("flush_workers", config.flush.workers as u64)
                .with_u64("compaction_workers", config.compaction.workers as u64)
                .with_u64("admin_workers", config.admin.workers as u64)
                .with_u64("idle_sleep_ms", config.idle_sleep_ms),
        );
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        let Some(stop) = &self.stop else {
            return Err(TridentError::MaintenanceRuntimeNotRunning);
        };
        stop.store(true, Ordering::Relaxed);
        slog::info("maintenance_runtime_stop_requested", slog::context());
        Ok(())
    }

    pub fn join(&mut self) -> Result<()> {
        if self.workers.is_empty() {
            return Err(TridentError::MaintenanceRuntimeNotRunning);
        }
        while let Some(worker) = self.workers.pop() {
            worker.handle.join().map_err(|_| {
                TridentError::TaskJoin("maintenance runtime worker panicked".to_string())
            })?;
        }
        self.stop = None;
        self.started_at_ms = None;
        self.config = None;
        slog::info("maintenance_runtime_stopped", slog::context());
        Ok(())
    }

    pub fn status(&self) -> RuntimeStatusSnapshot {
        let mut workers_by_lane = BTreeMap::new();
        for worker in &self.workers {
            let key = format!("{:?}", worker.lane).to_lowercase();
            *workers_by_lane.entry(key).or_insert(0usize) += 1;
        }
        RuntimeStatusSnapshot {
            running: self.is_running(),
            stop_requested: self
                .stop
                .as_ref()
                .map(|flag| flag.load(Ordering::Relaxed))
                .unwrap_or(false),
            started_at_ms: self.started_at_ms,
            workers_by_lane,
        }
    }

    fn is_running(&self) -> bool {
        !self.workers.is_empty()
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
