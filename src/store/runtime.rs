use super::{MaintenanceCycleOptions, StorageEngine};
use crate::errors::{Result, TridentError};
use crate::slog;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub type SharedStorageEngine = Arc<Mutex<StorageEngine>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageMaintenanceRuntimeConfig {
    pub workers: usize,
    pub idle_sleep_ms: u64,
    pub cycle: MaintenanceCycleOptions,
}

impl Default for StorageMaintenanceRuntimeConfig {
    fn default() -> Self {
        Self {
            workers: 1,
            idle_sleep_ms: 25,
            cycle: MaintenanceCycleOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StorageMaintenanceRuntimeStatus {
    pub running: bool,
    pub stop_requested: bool,
    pub started_at_ms: Option<u64>,
    pub workers: usize,
    pub completed_cycles: u64,
}

#[derive(Default)]
pub struct StorageMaintenanceRuntimeController {
    stop: Option<Arc<AtomicBool>>,
    started_at_ms: Option<u64>,
    workers: Vec<JoinHandle<()>>,
    completed_cycles: Arc<AtomicU64>,
    config: Option<StorageMaintenanceRuntimeConfig>,
}

impl StorageMaintenanceRuntimeController {
    pub fn start(
        &mut self,
        engine: SharedStorageEngine,
        config: StorageMaintenanceRuntimeConfig,
    ) -> Result<()> {
        if self.is_running() {
            return Err(TridentError::MaintenanceRuntimeRunning);
        }
        if config.workers == 0 {
            return Err(TridentError::InvalidConfig(
                "storage maintenance runtime requires at least one worker".to_string(),
            ));
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(stop.clone());
        self.started_at_ms = Some(now_millis());
        self.config = Some(config);
        self.completed_cycles.store(0, Ordering::Relaxed);

        for worker_idx in 0..config.workers {
            let stop_flag = stop.clone();
            let worker_engine = engine.clone();
            let completed_cycles = self.completed_cycles.clone();
            let idle_sleep_ms = config.idle_sleep_ms.max(1);
            let cycle_options = config.cycle;
            let handle = thread::Builder::new()
                .name(format!("trident-storage-maintenance-{worker_idx}"))
                .spawn(move || {
                    while !stop_flag.load(Ordering::Relaxed) {
                        let report = {
                            let mut engine = worker_engine.lock();
                            engine.run_maintenance_cycle_with_options(cycle_options)
                        };
                        match report {
                            Ok(report) => {
                                if report.executed.is_empty() {
                                    thread::sleep(Duration::from_millis(idle_sleep_ms));
                                } else {
                                    completed_cycles.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            Err(error) => {
                                slog::error(
                                    "storage_maintenance_runtime_worker_error",
                                    slog::context()
                                        .with_str("error", error.to_string())
                                        .with_bool("retryable", error.is_retryable()),
                                );
                                thread::sleep(Duration::from_millis(idle_sleep_ms));
                            }
                        }
                    }
                })
                .map_err(TridentError::Io)?;
            self.workers.push(handle);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        let Some(stop) = &self.stop else {
            return Err(TridentError::MaintenanceRuntimeNotRunning);
        };
        stop.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn join(&mut self) -> Result<()> {
        if self.workers.is_empty() {
            return Err(TridentError::MaintenanceRuntimeNotRunning);
        }
        while let Some(worker) = self.workers.pop() {
            worker.join().map_err(|_| {
                TridentError::TaskJoin("storage maintenance worker panicked".to_string())
            })?;
        }
        self.stop = None;
        self.started_at_ms = None;
        self.config = None;
        Ok(())
    }

    pub fn status(&self) -> StorageMaintenanceRuntimeStatus {
        StorageMaintenanceRuntimeStatus {
            running: self.is_running(),
            stop_requested: self
                .stop
                .as_ref()
                .map(|stop| stop.load(Ordering::Relaxed))
                .unwrap_or(false),
            started_at_ms: self.started_at_ms,
            workers: self.workers.len(),
            completed_cycles: self.completed_cycles.load(Ordering::Relaxed),
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
