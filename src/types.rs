use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;

pub type Key = Bytes;
pub type Value = Bytes;
pub type SequenceNumber = u64;
pub type TreeId = u32;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct ColumnFamily(pub String);

impl Default for ColumnFamily {
    fn default() -> Self {
        Self("default".to_string())
    }
}

impl fmt::Display for ColumnFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct ReadSnapshot {
    pub sequence: SequenceNumber,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueRef<'a> {
    Borrowed(&'a [u8]),
    Owned(Bytes),
}

impl<'a> ValueRef<'a> {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Owned(bytes) => bytes.as_ref(),
        }
    }

    pub fn into_cow(self) -> Cow<'a, [u8]> {
        match self {
            Self::Borrowed(bytes) => Cow::Borrowed(bytes),
            Self::Owned(bytes) => Cow::Owned(bytes.to_vec()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StoredValue {
    Put(Vec<u8>),
    PutWithExpiry { value: Vec<u8>, expires_at_ms: u64 },
    Merge(Vec<u8>),
    BlobPointer(ValuePointer),
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ValuePointer {
    pub path: String,
    pub offset: u64,
    pub len: u64,
    pub checksum: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VersionedValue {
    pub sequence: SequenceNumber,
    pub value: StoredValue,
}
