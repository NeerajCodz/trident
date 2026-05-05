use crate::transactions::BatchOp;
use crate::types::{ColumnFamily, Key, SequenceNumber, StoredValue, Value, VersionedValue};
use bytes::Bytes;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct MemTable {
    entries: BTreeMap<(ColumnFamily, Key), Vec<VersionedValue>>,
}

impl MemTable {
    pub fn apply(&mut self, sequence: SequenceNumber, op: &BatchOp) {
        match op {
            BatchOp::Put { cf, key, value } => {
                self.entries
                    .entry((cf.clone(), Bytes::from(key.clone())))
                    .or_default()
                    .push(VersionedValue {
                        sequence,
                        value: StoredValue::Put(value.clone()),
                    });
            }
            BatchOp::Delete { cf, key } => {
                self.entries
                    .entry((cf.clone(), Bytes::from(key.clone())))
                    .or_default()
                    .push(VersionedValue {
                        sequence,
                        value: StoredValue::Delete,
                    });
            }
        }
    }

    pub fn get(
        &self,
        cf: &ColumnFamily,
        key: &[u8],
        snapshot: SequenceNumber,
    ) -> Option<StoredValue> {
        self.entries
            .get(&(cf.clone(), Bytes::copy_from_slice(key)))
            .and_then(|versions| {
                versions
                    .iter()
                    .rev()
                    .find(|version| version.sequence <= snapshot)
                    .map(|version| version.value.clone())
            })
    }

    pub fn scan(
        &self,
        cf: &ColumnFamily,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        snapshot: SequenceNumber,
    ) -> Vec<(Key, Value)> {
        let mut out = Vec::new();
        for ((entry_cf, key), versions) in &self.entries {
            if entry_cf != cf {
                continue;
            }
            if start.is_some_and(|start| key.as_ref() < start) {
                continue;
            }
            if end.is_some_and(|end| key.as_ref() >= end) {
                continue;
            }
            if let Some(version) = versions
                .iter()
                .rev()
                .find(|version| version.sequence <= snapshot)
                && let StoredValue::Put(value) = &version.value
            {
                out.push((key.clone(), Bytes::from(value.clone())));
            }
        }
        out
    }

    pub fn drain_latest(&self) -> Vec<(ColumnFamily, Key, VersionedValue)> {
        self.entries
            .iter()
            .filter_map(|((cf, key), versions)| {
                versions
                    .last()
                    .cloned()
                    .map(|version| (cf.clone(), key.clone(), version))
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
