//! Benchmarking utilities and workload generators for Trident.
//!
//! This module provides:
//! - Workload generators (sequential, random, hot-key patterns)
//! - Latency distribution analysis
//! - Write amplification measurement
//! - Lock contention tracking
//! - Throughput measurement

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Workload pattern types
#[derive(Clone, Copy, Debug)]
pub enum WorkloadPattern {
    /// Sequential key generation (0, 1, 2, ...)
    Sequential,
    /// Uniform random keys
    UniformRandom,
    /// Hot-key pattern (80% of accesses hit 20% of keys)
    HotKey { hotset_fraction: f64 },
    /// Zipfian distribution (realistic workload)
    Zipfian { exponent: f64 },
}

/// Workload generator
pub struct WorkloadGenerator {
    pattern: WorkloadPattern,
    key_count: usize,
    rng_state: u64,
}

impl WorkloadGenerator {
    pub fn new(pattern: WorkloadPattern, key_count: usize) -> Self {
        Self {
            pattern,
            key_count,
            rng_state: 12345,
        }
    }

    /// Generate next key in the workload
    pub fn next_key(&mut self) -> usize {
        match self.pattern {
            WorkloadPattern::Sequential => {
                let key = self.rng_state as usize % self.key_count;
                self.rng_state = self.rng_state.wrapping_add(1);
                key
            }
            WorkloadPattern::UniformRandom => {
                self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                (self.rng_state as usize) % self.key_count
            }
            WorkloadPattern::HotKey { hotset_fraction } => {
                self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                let is_hot = (self.rng_state as f64 / u64::MAX as f64) < hotset_fraction;
                if is_hot {
                    let hot_keys = ((self.key_count as f64) * 0.2) as usize;
                    (self.rng_state as usize) % (hot_keys.max(1))
                } else {
                    let cold_start = ((self.key_count as f64) * 0.2) as usize;
                    cold_start + (self.rng_state as usize) % (self.key_count - cold_start)
                }
            }
            WorkloadPattern::Zipfian { exponent } => {
                // Simplified Zipfian: use rank-based probability
                self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                let u = (self.rng_state as f64) / (u64::MAX as f64);
                let rank = ((self.key_count as f64) * (1.0 - u.powf(1.0 / exponent))) as usize;
                rank.min(self.key_count - 1)
            }
        }
    }
}

/// Latency distribution histogram
pub struct LatencyDistribution {
    /// Buckets: [0-1ms, 1-10ms, 10-100ms, 100-1000ms, 1000+ms]
    buckets: [u64; 5],
    total_samples: u64,
    min_us: u64,
    max_us: u64,
}

impl LatencyDistribution {
    pub fn new() -> Self {
        Self {
            buckets: [0; 5],
            total_samples: 0,
            min_us: u64::MAX,
            max_us: 0,
        }
    }

    /// Record a latency sample in microseconds
    pub fn record(&mut self, latency_us: u64) {
        self.total_samples += 1;
        self.min_us = self.min_us.min(latency_us);
        self.max_us = self.max_us.max(latency_us);

        let bucket = match latency_us {
            0..=1_000 => 0,
            1_001..=10_000 => 1,
            10_001..=100_000 => 2,
            100_001..=1_000_000 => 3,
            _ => 4,
        };
        self.buckets[bucket] += 1;
    }

    /// Get percentile (0.0 to 1.0)
    pub fn percentile(&self, p: f64) -> u64 {
        let target = (self.total_samples as f64 * p) as u64;
        let mut cumulative = 0;

        for (i, &count) in self.buckets.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return match i {
                    0 => 500,       // mid-point of 0-1ms
                    1 => 5_500,     // mid-point of 1-10ms
                    2 => 55_000,    // mid-point of 10-100ms
                    3 => 550_000,   // mid-point of 100-1000ms
                    _ => 1_000_000, // mid-point of 1000+ms
                };
            }
        }
        self.max_us
    }

    pub fn min(&self) -> u64 {
        self.min_us
    }

    pub fn max(&self) -> u64 {
        self.max_us
    }

    pub fn avg(&self) -> u64 {
        if self.total_samples == 0 {
            0
        } else {
            let sum: u64 = self
                .buckets
                .iter()
                .enumerate()
                .map(|(i, &count)| {
                    let mid = match i {
                        0 => 500,
                        1 => 5_500,
                        2 => 55_000,
                        3 => 550_000,
                        _ => 1_000_000,
                    };
                    mid * count
                })
                .sum();
            sum / self.total_samples
        }
    }

    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    pub fn format_report(&self) -> String {
        format!(
            "Latency Distribution:\n  \
            Min: {}μs | Avg: {}μs | Max: {}μs\n  \
            P50: {}μs | P95: {}μs | P99: {}μs\n  \
            Buckets: [0-1ms]={} [1-10ms]={} [10-100ms]={} [100-1000ms]={} [1000+ms]={}\n  \
            Total samples: {}",
            self.min(),
            self.avg(),
            self.max(),
            self.percentile(0.50),
            self.percentile(0.95),
            self.percentile(0.99),
            self.buckets[0],
            self.buckets[1],
            self.buckets[2],
            self.buckets[3],
            self.buckets[4],
            self.total_samples
        )
    }
}

impl Default for LatencyDistribution {
    fn default() -> Self {
        Self::new()
    }
}

/// Write amplification tracker
pub struct WriteAmplificationTracker {
    logical_bytes_written: Arc<AtomicU64>,
    physical_bytes_written: Arc<AtomicU64>,
    wal_bytes_written: Arc<AtomicU64>,
}

impl WriteAmplificationTracker {
    pub fn new() -> Self {
        Self {
            logical_bytes_written: Arc::new(AtomicU64::new(0)),
            physical_bytes_written: Arc::new(AtomicU64::new(0)),
            wal_bytes_written: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record_logical_write(&self, bytes: u64) {
        self.logical_bytes_written
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_physical_write(&self, bytes: u64) {
        self.physical_bytes_written
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_wal_write(&self, bytes: u64) {
        self.wal_bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn logical_bytes(&self) -> u64 {
        self.logical_bytes_written.load(Ordering::Relaxed)
    }

    pub fn physical_bytes(&self) -> u64 {
        self.physical_bytes_written.load(Ordering::Relaxed)
    }

    pub fn wal_bytes(&self) -> u64 {
        self.wal_bytes_written.load(Ordering::Relaxed)
    }

    /// Compute write amplification factor
    pub fn write_amplification(&self) -> f64 {
        let logical = self.logical_bytes();
        if logical == 0 {
            0.0
        } else {
            self.physical_bytes() as f64 / logical as f64
        }
    }

    /// Compute WAL amplification
    pub fn wal_amplification(&self) -> f64 {
        let logical = self.logical_bytes();
        if logical == 0 {
            0.0
        } else {
            self.wal_bytes() as f64 / logical as f64
        }
    }

    pub fn total_amplification(&self) -> f64 {
        let logical = self.logical_bytes();
        if logical == 0 {
            0.0
        } else {
            (self.physical_bytes() + self.wal_bytes()) as f64 / logical as f64
        }
    }

    pub fn format_report(&self) -> String {
        format!(
            "Write Amplification:\n  \
            Logical: {} MB | Physical: {} MB | WAL: {} MB\n  \
            Physical Amp: {:.2}x | WAL Amp: {:.2}x | Total: {:.2}x",
            self.logical_bytes() / 1024 / 1024,
            self.physical_bytes() / 1024 / 1024,
            self.wal_bytes() / 1024 / 1024,
            self.write_amplification(),
            self.wal_amplification(),
            self.total_amplification()
        )
    }
}

impl Default for WriteAmplificationTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for WriteAmplificationTracker {
    fn clone(&self) -> Self {
        Self {
            logical_bytes_written: Arc::clone(&self.logical_bytes_written),
            physical_bytes_written: Arc::clone(&self.physical_bytes_written),
            wal_bytes_written: Arc::clone(&self.wal_bytes_written),
        }
    }
}

/// Lock contention tracker using timing-based detection
pub struct LockContentionTracker {
    total_lock_attempts: Arc<AtomicU64>,
    contended_acquisitions: Arc<AtomicU64>,
    total_wait_us: Arc<AtomicU64>,
}

impl LockContentionTracker {
    pub fn new() -> Self {
        Self {
            total_lock_attempts: Arc::new(AtomicU64::new(0)),
            contended_acquisitions: Arc::new(AtomicU64::new(0)),
            total_wait_us: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Record a lock acquisition with wait time
    pub fn record_acquisition(&self, wait_us: u64) {
        self.total_lock_attempts.fetch_add(1, Ordering::Relaxed);
        if wait_us > 100 {
            // Contended if wait > 100μs
            self.contended_acquisitions.fetch_add(1, Ordering::Relaxed);
        }
        self.total_wait_us.fetch_add(wait_us, Ordering::Relaxed);
    }

    pub fn total_attempts(&self) -> u64 {
        self.total_lock_attempts.load(Ordering::Relaxed)
    }

    pub fn contended_attempts(&self) -> u64 {
        self.contended_acquisitions.load(Ordering::Relaxed)
    }

    pub fn avg_wait_us(&self) -> u64 {
        let total_attempts = self.total_attempts();
        if total_attempts == 0 {
            0
        } else {
            self.total_wait_us.load(Ordering::Relaxed) / total_attempts
        }
    }

    pub fn contention_rate(&self) -> f64 {
        let total = self.total_attempts();
        if total == 0 {
            0.0
        } else {
            self.contended_attempts() as f64 / total as f64
        }
    }

    pub fn format_report(&self) -> String {
        format!(
            "Lock Contention:\n  \
            Total attempts: {} | Contended: {} ({:.1}%)\n  \
            Avg wait: {}μs",
            self.total_attempts(),
            self.contended_attempts(),
            self.contention_rate() * 100.0,
            self.avg_wait_us()
        )
    }
}

impl Default for LockContentionTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for LockContentionTracker {
    fn clone(&self) -> Self {
        Self {
            total_lock_attempts: Arc::clone(&self.total_lock_attempts),
            contended_acquisitions: Arc::clone(&self.contended_acquisitions),
            total_wait_us: Arc::clone(&self.total_wait_us),
        }
    }
}

/// Throughput measurement helper
pub struct ThroughputMeter {
    start_time: Instant,
    operation_count: Arc<AtomicU64>,
    byte_count: Arc<AtomicU64>,
}

impl ThroughputMeter {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            operation_count: Arc::new(AtomicU64::new(0)),
            byte_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record_operation(&self, bytes: u64) {
        self.operation_count.fetch_add(1, Ordering::Relaxed);
        self.byte_count.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn ops_per_sec(&self) -> f64 {
        let elapsed_secs = self.start_time.elapsed().as_secs_f64();
        if elapsed_secs == 0.0 {
            0.0
        } else {
            self.operation_count.load(Ordering::Relaxed) as f64 / elapsed_secs
        }
    }

    pub fn mb_per_sec(&self) -> f64 {
        let elapsed_secs = self.start_time.elapsed().as_secs_f64();
        if elapsed_secs == 0.0 {
            0.0
        } else {
            (self.byte_count.load(Ordering::Relaxed) as f64 / 1024.0 / 1024.0) / elapsed_secs
        }
    }

    pub fn format_report(&self) -> String {
        format!(
            "Throughput:\n  \
            Operations: {} ops/sec ({} total)\n  \
            Bandwidth: {:.2} MB/sec ({} MB total)\n  \
            Time: {:.2}s",
            self.ops_per_sec() as u64,
            self.operation_count.load(Ordering::Relaxed),
            self.mb_per_sec(),
            self.byte_count.load(Ordering::Relaxed) / 1024 / 1024,
            self.start_time.elapsed().as_secs_f64()
        )
    }
}

impl Default for ThroughputMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ThroughputMeter {
    fn clone(&self) -> Self {
        Self {
            start_time: self.start_time,
            operation_count: Arc::clone(&self.operation_count),
            byte_count: Arc::clone(&self.byte_count),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_sequential() {
        let mut generator = WorkloadGenerator::new(WorkloadPattern::Sequential, 10);
        let mut last_key = generator.next_key();
        for _ in 0..9 {
            let key = generator.next_key();
            // In sequential pattern, keys should increment mod key_count
            assert_eq!(key, (last_key + 1) % 10);
            last_key = key;
        }
    }

    #[test]
    fn workload_uniform_random() {
        let mut generator = WorkloadGenerator::new(WorkloadPattern::UniformRandom, 100);
        let keys: Vec<_> = (0..50).map(|_| generator.next_key()).collect();
        // Should hit various keys
        assert!(keys.iter().max() > Some(&20));
    }

    #[test]
    fn latency_distribution_buckets() {
        let mut dist = LatencyDistribution::new();
        dist.record(500); // 0-1ms
        dist.record(5_000); // 1-10ms
        dist.record(50_000); // 10-100ms

        assert_eq!(dist.min(), 500);
        assert_eq!(dist.max(), 50_000);
        assert_eq!(dist.total_samples(), 3);
        assert!(dist.avg() > 500);
    }

    #[test]
    fn write_amplification() {
        let tracker = WriteAmplificationTracker::new();
        tracker.record_logical_write(1000);
        tracker.record_physical_write(2000);
        tracker.record_wal_write(500);

        assert_eq!(tracker.logical_bytes(), 1000);
        assert_eq!(tracker.physical_bytes(), 2000);
        assert_eq!(tracker.write_amplification(), 2.0);
    }

    #[test]
    fn throughput_meter() {
        let meter = ThroughputMeter::new();
        for _ in 0..100 {
            meter.record_operation(1024);
        }

        assert_eq!(meter.operation_count.load(Ordering::Relaxed), 100);
        assert!(meter.ops_per_sec() > 0.0);
    }

    #[test]
    fn lock_contention_tracking() {
        let tracker = LockContentionTracker::new();
        tracker.record_acquisition(50); // uncontended
        tracker.record_acquisition(200); // contended
        tracker.record_acquisition(50); // uncontended

        assert_eq!(tracker.total_attempts(), 3);
        assert_eq!(tracker.contended_attempts(), 1);
        assert!(tracker.contention_rate() > 0.0);
    }
}
