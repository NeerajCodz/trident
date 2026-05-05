use crate::config::TridentConfig;
use crate::engine::TridentEngine;
use crate::errors::Result;
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
            .expect("trident open task panicked")?;
        Ok(Self { inner })
    }

    pub async fn put(&self, key: Key, value: Value) -> Result<SequenceNumber> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.put(key, value))
            .await
            .expect("trident put task panicked")
    }

    pub async fn get(&self, key: Key) -> Result<Option<Value>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.get(key))
            .await
            .expect("trident get task panicked")
    }

    pub async fn write_batch(&self, batch: WriteBatch) -> Result<SequenceNumber> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.write_batch(batch))
            .await
            .expect("trident write task panicked")
    }

    pub async fn flush(&self) -> Result<Option<u64>> {
        let engine = self.inner.clone();
        tokio::task::spawn_blocking(move || engine.flush())
            .await
            .expect("trident flush task panicked")
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        self.inner.snapshot()
    }
}
