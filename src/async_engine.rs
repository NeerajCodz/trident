use crate::config::TridentConfig;
use crate::engine::TridentEngine;
use crate::errors::{Result, TridentError};
use crate::transactions::WriteBatch;
use crate::types::{Key, ReadSnapshot, SequenceNumber, Value};

#[derive(Clone)]
pub struct AsyncTridentEngine {
    inner: TridentEngine,
}

impl AsyncTridentEngine {
    pub async fn open(config: TridentConfig) -> Result<Self> {
        let inner = tokio::task::spawn_blocking(move || TridentEngine::open(config))
            .await
            .map_err(|error| TridentError::TaskJoin(error.to_string()))??;
        Ok(Self { inner })
    }

    pub async fn put(&self, key: Key, value: Value) -> Result<SequenceNumber> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.put(key, value))
            .await
            .map_err(|error| TridentError::TaskJoin(error.to_string()))?
    }

    pub async fn get(&self, key: Key) -> Result<Option<Value>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.get(key))
            .await
            .map_err(|error| TridentError::TaskJoin(error.to_string()))?
    }

    pub async fn write_batch(&self, batch: WriteBatch) -> Result<SequenceNumber> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.write_batch(batch))
            .await
            .map_err(|error| TridentError::TaskJoin(error.to_string()))?
    }

    pub async fn flush(&self) -> Result<Option<u64>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.flush())
            .await
            .map_err(|error| TridentError::TaskJoin(error.to_string()))?
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        self.inner.snapshot()
    }
}
