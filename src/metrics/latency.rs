//! Latency tracking and percentile computation for storage operations.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Tracks latency samples for a specific operation type.
///
/// Uses a fixed-size ring buffer to avoid unbounded memory growth.
/// Thread-safe via atomic operations and interior mutability.
pub struct LatencyTracker {
    /// Ring buffer of latency samples (in microseconds)
    samples: Arc<parking_lot::Mutex<Vec<u64>>>,
    capacity: usize,
    index: AtomicU64,
    count: AtomicU64,
}

impl LatencyTracker {
    /// Create a new latency tracker with specified sample capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: Arc::new(parking_lot::Mutex::new(Vec::with_capacity(capacity))),
            capacity,
            index: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Record a latency sample in microseconds.
    pub fn record(&self, micros: u64) {
        let mut samples = self.samples.lock();
        let idx = (self.index.fetch_add(1, Ordering::Relaxed) as usize) % self.capacity;

        if idx >= samples.len() {
            samples.push(micros);
        } else {
            samples[idx] = micros;
        }

        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the total number of samples recorded.
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Compute percentile latency (e.g., 0.50 for p50, 0.99 for p99).
    ///
    /// Returns latency in microseconds.
    pub fn percentile(&self, p: f64) -> Option<u64> {
        if !(0.0..=1.0).contains(&p) {
            return None;
        }

        let samples = self.samples.lock();
        if samples.is_empty() {
            return None;
        }

        let mut sorted = samples.clone();
        sorted.sort_unstable();

        let idx = ((p * (sorted.len() as f64)) as usize).min(sorted.len() - 1);
        Some(sorted[idx])
    }

    /// Get minimum latency recorded.
    pub fn min(&self) -> Option<u64> {
        let samples = self.samples.lock();
        samples.iter().copied().min()
    }

    /// Get maximum latency recorded.
    pub fn max(&self) -> Option<u64> {
        let samples = self.samples.lock();
        samples.iter().copied().max()
    }

    /// Get average latency recorded.
    pub fn avg(&self) -> Option<u64> {
        let samples = self.samples.lock();
        if samples.is_empty() {
            return None;
        }
        let sum: u64 = samples.iter().sum();
        Some(sum / samples.len() as u64)
    }
}

impl Clone for LatencyTracker {
    fn clone(&self) -> Self {
        Self {
            samples: Arc::clone(&self.samples),
            capacity: self.capacity,
            index: AtomicU64::new(self.index.load(Ordering::Relaxed)),
            count: AtomicU64::new(self.count.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_tracker_records_samples() {
        let tracker = LatencyTracker::new(1000);
        tracker.record(100);
        tracker.record(200);
        tracker.record(150);

        assert_eq!(tracker.count(), 3);
        assert_eq!(tracker.min(), Some(100));
        assert_eq!(tracker.max(), Some(200));
    }

    #[test]
    fn latency_tracker_percentiles() {
        let tracker = LatencyTracker::new(100);
        for i in 1..=100 {
            tracker.record(i);
        }

        // For p=0.5 with 100 samples [1..100], index = floor(0.5*100) = 50
        // sorted[50] = 51 (0-indexed)
        assert_eq!(tracker.percentile(0.50), Some(51));
        assert!(tracker.percentile(0.95).unwrap() >= 94);
        assert!(tracker.percentile(0.99).unwrap() >= 98);
    }

    #[test]
    fn latency_tracker_wraps_buffer() {
        let tracker = LatencyTracker::new(5);
        for i in 0..10 {
            tracker.record(i as u64);
        }

        // All 10 recorded, but only 5 stored (ring buffer)
        assert_eq!(tracker.count(), 10);
    }
}
