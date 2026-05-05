use bytes::Bytes;

use crate::engine::core::engine::TridentEngine;
use crate::types::ColumnFamily;

impl TridentEngine {
    pub(crate) fn cache_insert_with_partition(
        &self,
        cf: &ColumnFamily,
        cache_key: (String, u64, Vec<u8>),
        value: Bytes,
    ) {
        let options = self.cf_options_map();
        let partition_percent = options
            .get(&cf.0)
            .and_then(|entry| entry.cache_partition_percent)
            .unwrap_or(100) as usize;
        let partition_budget = self
            .inner
            .config
            .cache_size_bytes
            .saturating_mul(partition_percent)
            / 100;
        let mut cache = self.inner.cache.lock();
        let replaced_len = cache.value_len(&cache_key).unwrap_or(0);
        let mut by_cf = self.inner.cache_bytes_by_cf.lock();
        let current_for_cf = *by_cf.get(&cf.0).unwrap_or(&0);
        let projected_cf = current_for_cf
            .saturating_sub(replaced_len)
            .saturating_add(value.len());
        if value.len() > partition_budget || projected_cf > partition_budget {
            self.inner
                .metrics
                .cache_admission_rejects
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        let evicted = cache.insert_with_evicted(cache_key, value.clone());
        by_cf.insert(cf.0.clone(), projected_cf);
        for ((evicted_cf, _, _), evicted_value) in evicted {
            if let Some(evicted_bytes) = by_cf.get_mut(&evicted_cf) {
                *evicted_bytes = evicted_bytes.saturating_sub(evicted_value.len());
            }
        }
    }
}
