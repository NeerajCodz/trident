use bytes::Bytes;
use std::collections::BTreeMap;

use crate::engine::core::engine::TridentEngine;
use crate::errors::Result;
use crate::types::{ColumnFamily, Key, ReadSnapshot, Value};

impl TridentEngine {
    pub fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<(Key, Value)>> {
        self.scan_cf_at_snapshot(&ColumnFamily::default(), start, end, limit, self.snapshot())
    }

    pub fn scan_prefix(&self, prefix: &[u8], limit: usize) -> Result<Vec<(Key, Value)>> {
        self.scan_prefix_cf_at_snapshot(&ColumnFamily::default(), prefix, limit, self.snapshot())
    }

    pub fn scan_at_snapshot(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        limit: usize,
        snapshot: ReadSnapshot,
    ) -> Result<Vec<(Key, Value)>> {
        self.scan_cf_at_snapshot(&ColumnFamily::default(), start, end, limit, snapshot)
    }

    pub fn scan_prefix_at_snapshot(
        &self,
        prefix: &[u8],
        limit: usize,
        snapshot: ReadSnapshot,
    ) -> Result<Vec<(Key, Value)>> {
        self.scan_prefix_cf_at_snapshot(&ColumnFamily::default(), prefix, limit, snapshot)
    }

    pub fn scan_cf(
        &self,
        cf: &ColumnFamily,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<(Key, Value)>> {
        self.scan_cf_at_snapshot(cf, start, end, limit, self.snapshot())
    }

    pub fn scan_prefix_cf(
        &self,
        cf: &ColumnFamily,
        prefix: &[u8],
        limit: usize,
    ) -> Result<Vec<(Key, Value)>> {
        self.scan_prefix_cf_at_snapshot(cf, prefix, limit, self.snapshot())
    }

    pub fn scan_cf_at_snapshot(
        &self,
        cf: &ColumnFamily,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        limit: usize,
        snapshot: ReadSnapshot,
    ) -> Result<Vec<(Key, Value)>> {
        self.ensure_column_family(cf)?;
        let mut rows = BTreeMap::new();
        for (key, value) in self
            .inner
            .memtable
            .lock()
            .scan(cf, start, end, snapshot.sequence)
        {
            rows.insert(key, value);
        }
        for ((entry_cf, key), versions) in self.inner.segment_index.lock().iter() {
            if entry_cf != cf {
                continue;
            }
            if start.is_some_and(|start| key.as_ref() < start) {
                continue;
            }
            if end.is_some_and(|end| key.as_ref() >= end) {
                continue;
            }
            if rows.contains_key(key) {
                continue;
            }
            if let Some(value) = self.resolve_versions_chain(cf, versions, snapshot.sequence)? {
                rows.insert(key.clone(), Bytes::from(value));
            }
        }
        Ok(rows.into_iter().take(limit).collect())
    }

    pub fn scan_prefix_cf_at_snapshot(
        &self,
        cf: &ColumnFamily,
        prefix: &[u8],
        limit: usize,
        snapshot: ReadSnapshot,
    ) -> Result<Vec<(Key, Value)>> {
        let end = prefix_upper_bound(prefix);
        self.scan_cf_at_snapshot(cf, Some(prefix), end.as_deref(), limit, snapshot)
    }

    pub fn snapshot(&self) -> ReadSnapshot {
        self.inner.snapshots.snapshot()
    }

    pub fn pin_snapshot(&self) -> crate::ram::PinnedSnapshot {
        self.inner.snapshots.pin()
    }
}

fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
    if prefix.is_empty() {
        return None;
    }
    let mut upper = prefix.to_vec();
    for i in (0..upper.len()).rev() {
        if upper[i] != u8::MAX {
            upper[i] = upper[i].saturating_add(1);
            upper.truncate(i + 1);
            return Some(upper);
        }
    }
    None
}
