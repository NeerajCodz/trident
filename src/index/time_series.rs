use crate::store::RecordId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A single time-series data point.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp_ms: i64,
    pub rid: RecordId,
}

/// A time partition holds points within a time range.
/// Points are sorted by timestamp for efficient range queries.
#[derive(Clone, Debug)]
pub struct TimePartition {
    pub start_ms: i64,
    pub end_ms: i64,
    points: Vec<TimeSeriesPoint>,
}

impl TimePartition {
    pub fn new(start_ms: i64, end_ms: i64) -> Self {
        Self {
            start_ms,
            end_ms,
            points: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Insert a point. Returns false if timestamp is outside partition range.
    pub fn insert(&mut self, timestamp_ms: i64, rid: RecordId) -> bool {
        if timestamp_ms < self.start_ms || timestamp_ms > self.end_ms {
            return false;
        }
        let point = TimeSeriesPoint { timestamp_ms, rid };
        // Insert in sorted order
        let pos = self
            .points
            .binary_search_by(|p| p.timestamp_ms.cmp(&timestamp_ms))
            .unwrap_or_else(|i| i);
        self.points.insert(pos, point);
        true
    }

    /// Query all points within [start_ms, end_ms].
    pub fn range(&self, start_ms: i64, end_ms: i64) -> Vec<RecordId> {
        let start_pos = self.points.partition_point(|p| p.timestamp_ms < start_ms);
        self.points[start_pos..]
            .iter()
            .take_while(|p| p.timestamp_ms <= end_ms)
            .map(|p| p.rid)
            .collect()
    }

    /// Query the most recent N points.
    pub fn latest(&self, n: usize) -> Vec<RecordId> {
        self.points.iter().rev().take(n).map(|p| p.rid).collect()
    }

    /// Query the oldest N points.
    pub fn oldest(&self, n: usize) -> Vec<RecordId> {
        self.points.iter().take(n).map(|p| p.rid).collect()
    }

    /// Count points in a time range.
    pub fn count_range(&self, start_ms: i64, end_ms: i64) -> usize {
        let start_pos = self.points.partition_point(|p| p.timestamp_ms < start_ms);
        let end_pos = self.points.partition_point(|p| p.timestamp_ms <= end_ms);
        end_pos - start_pos
    }
}

/// Manages multiple time partitions with automatic partitioning and retention.
///
/// Partitions are created based on a duration (e.g., 1 hour, 1 day).
/// Old partitions can be dropped based on retention policy.
#[derive(Debug)]
pub struct TimeSeriesIndex {
    partition_duration_ms: i64,
    retention_ms: Option<i64>,
    partitions: BTreeMap<i64, TimePartition>, // keyed by partition start_ms
}

impl Default for TimeSeriesIndex {
    fn default() -> Self {
        Self {
            partition_duration_ms: 3600 * 1000, // 1 hour default
            retention_ms: None,
            partitions: BTreeMap::new(),
        }
    }
}

impl TimeSeriesIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set partition duration in milliseconds.
    pub fn with_partition_duration(mut self, duration_ms: i64) -> Self {
        self.partition_duration_ms = duration_ms;
        self
    }

    /// Set retention period in milliseconds. Partitions older than this are dropped.
    pub fn with_retention(mut self, retention_ms: i64) -> Self {
        self.retention_ms = Some(retention_ms);
        self
    }

    pub fn len(&self) -> usize {
        self.partitions.values().map(|p| p.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.partitions.values().all(|p| p.is_empty())
    }

    pub fn partition_count(&self) -> usize {
        self.partitions.len()
    }

    /// Insert a data point at the given timestamp.
    pub fn insert(&mut self, timestamp_ms: i64, rid: RecordId) {
        let partition_start = self.partition_key(timestamp_ms);
        let partition = self.partitions.entry(partition_start).or_insert_with(|| {
            TimePartition::new(
                partition_start,
                partition_start + self.partition_duration_ms,
            )
        });
        partition.insert(timestamp_ms, rid);

        // Apply retention policy
        self.apply_retention();
    }

    /// Query all points within [start_ms, end_ms] across all partitions.
    pub fn range(&self, start_ms: i64, end_ms: i64) -> Vec<RecordId> {
        let mut results = Vec::new();
        // Only scan partitions that could contain points in the range
        for (_, partition) in self.partitions.range(..=end_ms) {
            if partition.end_ms < start_ms {
                continue;
            }
            results.extend(partition.range(start_ms, end_ms));
        }
        results
    }

    /// Query the most recent N points across all partitions.
    pub fn latest(&self, n: usize) -> Vec<RecordId> {
        let mut results = Vec::new();
        for (_, partition) in self.partitions.iter().rev() {
            let needed = n - results.len();
            if needed == 0 {
                break;
            }
            results.extend(partition.latest(needed));
        }
        results
    }

    /// Count points in a time range.
    pub fn count_range(&self, start_ms: i64, end_ms: i64) -> usize {
        let mut count = 0;
        for (_, partition) in self.partitions.range(..=end_ms) {
            if partition.end_ms < start_ms {
                continue;
            }
            count += partition.count_range(start_ms, end_ms);
        }
        count
    }

    /// Drop partitions older than the retention period.
    pub fn apply_retention(&mut self) {
        if let Some(retention_ms) = self.retention_ms {
            let now = now_ms();
            let cutoff = now - retention_ms;
            self.partitions.retain(|start, _| *start >= cutoff);
        }
    }

    /// Manually drop partitions older than a given timestamp.
    pub fn drop_before(&mut self, cutoff_ms: i64) {
        self.partitions.retain(|start, _| *start >= cutoff_ms);
    }

    fn partition_key(&self, timestamp_ms: i64) -> i64 {
        (timestamp_ms / self.partition_duration_ms) * self.partition_duration_ms
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
