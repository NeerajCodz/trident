use crate::types::{ColumnFamily, Key, VersionedValue};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentEntry {
    pub cf: ColumnFamily,
    pub key: Key,
    pub version: VersionedValue,
}
