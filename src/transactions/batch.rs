use crate::types::{ColumnFamily, Key, Value};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BatchOp {
    Put {
        cf: ColumnFamily,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        cf: ColumnFamily,
        key: Vec<u8>,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteBatch {
    ops: Vec<BatchOp>,
}

impl WriteBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(
        &mut self,
        cf: impl Into<ColumnFamily>,
        key: impl Into<Key>,
        value: impl Into<Value>,
    ) -> &mut Self {
        self.ops.push(BatchOp::Put {
            cf: cf.into(),
            key: key.into().to_vec(),
            value: value.into().to_vec(),
        });
        self
    }

    pub fn put_default(&mut self, key: impl Into<Bytes>, value: impl Into<Bytes>) -> &mut Self {
        self.put(ColumnFamily::default(), key, value)
    }

    pub fn delete(&mut self, cf: impl Into<ColumnFamily>, key: impl Into<Key>) -> &mut Self {
        self.ops.push(BatchOp::Delete {
            cf: cf.into(),
            key: key.into().to_vec(),
        });
        self
    }

    pub fn delete_default(&mut self, key: impl Into<Bytes>) -> &mut Self {
        self.delete(ColumnFamily::default(), key)
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn ops(&self) -> &[BatchOp] {
        &self.ops
    }
}

impl From<&str> for ColumnFamily {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}
