use crate::errors::Result;
use crate::transactions::WriteBatch;
use crate::types::{Key, ReadSnapshot, SequenceNumber, Value};

pub trait PoiesisStorageAdapter: Send + Sync {
    fn put_page_value(&self, key: Key, value: Value) -> Result<SequenceNumber>;
    fn read_page_value(&self, key: &[u8], snapshot: ReadSnapshot) -> Result<Option<Value>>;
    fn write_kv_batch(&self, batch: WriteBatch) -> Result<SequenceNumber>;
    fn flush_durable(&self) -> Result<()>;
}
