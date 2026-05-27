use crate::config::PraxisConfig;
use crate::errors::{PraxisError, Result};
use crate::maintenance::{MaintenanceRuntimeConfig, MaintenanceStatusSnapshot};
use crate::transactions::WriteBatch;
use crate::types::{Key, ReadSnapshot, SequenceNumber, Value};
use std::path::PathBuf;

use super::PraxisEngine;

#[derive(Clone)]
pub struct AsyncPraxisEngine {
    inner: PraxisEngine,
}

impl AsyncPraxisEngine {
    pub async fn open(config: PraxisConfig) -> Result<Self> {
        let inner = tokio::task::spawn_blocking(move || PraxisEngine::open(config))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))??;
        Ok(Self { inner })
    }

    pub async fn put(&self, key: Key, value: Value) -> Result<SequenceNumber> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.put(key, value))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn get(&self, key: Key) -> Result<Option<Value>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.get(key))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn write_batch(&self, batch: WriteBatch) -> Result<SequenceNumber> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.write_batch(batch))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn flush(&self) -> Result<Option<u64>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.flush())
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn compact(&self) -> Result<u64> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.compact())
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn checkpoint(&self) -> Result<crate::manifest::CheckpointMetadata> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.checkpoint())
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn backup_to(&self, path: PathBuf) -> Result<()> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.backup_to(path))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn scan_prefix(&self, prefix: Key, limit: usize) -> Result<Vec<(Key, Value)>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.scan_prefix(&prefix, limit))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn scan_prefix_at_snapshot(
        &self,
        prefix: Key,
        limit: usize,
        snapshot: ReadSnapshot,
    ) -> Result<Vec<(Key, Value)>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            engine.scan_prefix_at_snapshot(&prefix, limit, snapshot)
        })
        .await
        .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        self.inner.snapshot()
    }

    pub async fn start_maintenance_runtime(&self, config: MaintenanceRuntimeConfig) -> Result<()> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.start_maintenance_runtime(config))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn stop_maintenance_runtime(&self) -> Result<()> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            engine.stop_maintenance_runtime()?;
            engine.join_maintenance_runtime()
        })
        .await
        .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }

    pub async fn maintenance_status(&self) -> Result<MaintenanceStatusSnapshot> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.maintenance_status())
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))
    }

    pub async fn retry_maintenance_job(&self, job_id: u64) -> Result<u64> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.retry_maintenance_job(job_id))
            .await
            .map_err(|error| PraxisError::TaskJoin(error.to_string()))?
    }
}
