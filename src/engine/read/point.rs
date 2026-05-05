use bytes::Bytes;

use crate::engine::core::engine::TridentEngine;
use crate::errors::Result;
use crate::segments::bloom::bloom_key;
use crate::types::{ColumnFamily, ReadSnapshot, Value, ValueRef};
use std::sync::atomic::Ordering;

impl TridentEngine {
    pub fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Value>> {
        self.get_cf(&ColumnFamily::default(), key.as_ref(), self.snapshot())
    }

    pub fn get_cf(
        &self,
        cf: &ColumnFamily,
        key: &[u8],
        snapshot: ReadSnapshot,
    ) -> Result<Option<Value>> {
        self.ensure_column_family(cf)?;
        self.inner.metrics.reads.fetch_add(1, Ordering::Relaxed);
        if let Some(value) = self.resolve_value_at_snapshot(cf, key, snapshot.sequence)? {
            return Ok(Some(Bytes::from(value)));
        }
        let cache_key = (cf.0.clone(), snapshot.sequence, key.to_vec());
        if let Some(value) = self.inner.cache.lock().get(&cache_key) {
            self.inner
                .metrics
                .cache_hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(Some(value));
        }
        self.inner
            .metrics
            .cache_misses
            .fetch_add(1, Ordering::Relaxed);
        if !self.segment_filters_may_contain(cf, key) {
            self.inner
                .metrics
                .bloom_negative_hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }
        let resolved = self.resolve_value_at_snapshot(cf, key, snapshot.sequence)?;
        Ok(match resolved {
            Some(value) => {
                let value = Bytes::from(value);
                self.cache_insert_with_partition(cf, cache_key, value.clone());
                Some(value)
            }
            None => None,
        })
    }

    pub fn get_ref(&self, key: impl AsRef<[u8]>) -> Result<Option<ValueRef<'static>>> {
        Ok(self.get(key)?.map(ValueRef::Owned))
    }

    pub(crate) fn segment_filters_may_contain(&self, cf: &ColumnFamily, key: &[u8]) -> bool {
        let encoded = bloom_key(&cf.0, key);
        let prefix_len = self
            .cf_options_map()
            .get(&cf.0)
            .and_then(|options| options.prefix_extractor_len);
        self.inner.manifest.lock().segments.iter().any(|segment| {
            if !segment.min_key.is_empty()
                && (key < segment.min_key.as_slice() || key > segment.max_key.as_slice())
            {
                return false;
            }
            if let (Some(filter), Some(expected_prefix_len)) =
                (&segment.partitioned_bloom_filter, prefix_len)
                && filter.prefix_len == expected_prefix_len
            {
                return filter.may_contain(&encoded);
            }
            segment.bloom_filter.may_contain(&encoded)
        })
    }
}
