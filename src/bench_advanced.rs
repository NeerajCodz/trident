//! Advanced benchmarking framework for Trident - Phase 3B
//!
//! This module provides production-grade benchmarking infrastructure:
//! - Performance result aggregation and statistical analysis
//! - Workload-specific metrics collection
//! - Benchmark result reporting and comparison
//! - Performance regression detection

use std::fmt;
use serde::{Serialize, Deserialize};
use crate::bench::{LatencyDistribution, WorkloadPattern, WriteAmplificationTracker, LockContentionTracker, ThroughputMeter};

/// Benchmark result for a single test run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub workload_pattern: String,
    pub duration_secs: f64,
    pub latency_min_us: u64,
    pub latency_max_us: u64,
    pub latency_avg_us: u64,
    pub latency_p50_us: u64,
    pub latency_p95_us: u64,
    pub latency_p99_us: u64,
    pub throughput_ops_per_sec: u64,
    pub throughput_mb_per_sec: f64,
    pub write_amp_physical: f64,
    pub write_amp_wal: f64,
    pub write_amp_total: f64,
    pub lock_contention_rate: f64,
    pub lock_avg_wait_us: u64,
}

impl BenchmarkResult {
    /// Create result from collected metrics
    pub fn from_metrics(
        name: impl Into<String>,
        pattern: &WorkloadPattern,
        duration_secs: f64,
        latency: &LatencyDistribution,
        throughput: &ThroughputMeter,
        amplification: &WriteAmplificationTracker,
        contention: &LockContentionTracker,
    ) -> Self {
        Self {
            name: name.into(),
            workload_pattern: format!("{:?}", pattern),
            duration_secs,
            latency_min_us: latency.min(),
            latency_max_us: latency.max(),
            latency_avg_us: latency.avg(),
            latency_p50_us: latency.percentile(0.50),
            latency_p95_us: latency.percentile(0.95),
            latency_p99_us: latency.percentile(0.99),
            throughput_ops_per_sec: throughput.ops_per_sec() as u64,
            throughput_mb_per_sec: throughput.mb_per_sec(),
            write_amp_physical: amplification.write_amplification(),
            write_amp_wal: amplification.wal_amplification(),
            write_amp_total: amplification.total_amplification(),
            lock_contention_rate: contention.contention_rate(),
            lock_avg_wait_us: contention.avg_wait_us(),
        }
    }
}

impl fmt::Display for BenchmarkResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Benchmark: {} ({})\n  \
             Duration: {:.2}s\n  \
             Latency: min={}μs avg={}μs p50={}μs p95={}μs p99={}μs max={}μs\n  \
             Throughput: {} ops/s {:.2} MB/s\n  \
             Write Amp: physical={:.2}x wal={:.2}x total={:.2}x\n  \
             Lock Contention: {:.1}% (avg_wait={}μs)",
            self.name,
            self.workload_pattern,
            self.duration_secs,
            self.latency_min_us,
            self.latency_avg_us,
            self.latency_p50_us,
            self.latency_p95_us,
            self.latency_p99_us,
            self.latency_max_us,
            self.throughput_ops_per_sec,
            self.throughput_mb_per_sec,
            self.write_amp_physical,
            self.write_amp_wal,
            self.write_amp_total,
            self.lock_contention_rate * 100.0,
            self.lock_avg_wait_us
        )
    }
}

/// Benchmark suite for aggregating multiple runs
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BenchmarkSuite {
    pub results: Vec<BenchmarkResult>,
    pub timestamp: String,
}

impl BenchmarkSuite {
    pub fn new() -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            results: Vec::new(),
            timestamp: now,
        }
    }

    pub fn add_result(&mut self, result: BenchmarkResult) {
        self.results.push(result);
    }

    /// Compute statistics across all results
    pub fn statistics(&self) -> BenchmarkStatistics {
        BenchmarkStatistics::from_results(&self.results)
    }

    /// Generate JSON report
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Compare results between two test runs (baseline vs current)
    pub fn compare(&self, baseline: &BenchmarkSuite) -> Vec<PerformanceRegression> {
        let mut regressions = Vec::new();

        for current_result in &self.results {
            if let Some(baseline_result) = baseline.results.iter()
                .find(|r| r.name == current_result.name && r.workload_pattern == current_result.workload_pattern) {
                
                let mut regression = PerformanceRegression {
                    benchmark_name: current_result.name.clone(),
                    workload: current_result.workload_pattern.clone(),
                    metrics: Vec::new(),
                };

                // Check each metric for regression (>10% worse)
                macro_rules! check_metric {
                    ($field:ident, $display:expr, higher_is_worse: true) => {
                        let current = current_result.$field as f64;
                        let baseline = baseline_result.$field as f64;
                        if baseline > 0.0 {
                            let change = (current - baseline) / baseline * 100.0;
                            if change > 10.0 {
                                regression.metrics.push(MetricRegression {
                                    metric: $display.into(),
                                    baseline_value: baseline,
                                    current_value: current,
                                    change_percent: change,
                                    is_regression: true,
                                });
                            }
                        }
                    };
                    ($field:ident, $display:expr, higher_is_worse: false) => {
                        let current = current_result.$field as f64;
                        let baseline = baseline_result.$field as f64;
                        if baseline > 0.0 {
                            let change = (baseline - current) / baseline * 100.0;
                            if change > 10.0 {
                                regression.metrics.push(MetricRegression {
                                    metric: $display.into(),
                                    baseline_value: baseline,
                                    current_value: current,
                                    change_percent: -change,
                                    is_regression: true,
                                });
                            }
                        }
                    };
                }

                check_metric!(latency_p99_us, "P99 Latency", higher_is_worse: true);
                check_metric!(latency_p95_us, "P95 Latency", higher_is_worse: true);
                check_metric!(throughput_ops_per_sec, "Throughput (ops/s)", higher_is_worse: false);
                check_metric!(write_amp_total, "Total Write Amplification", higher_is_worse: true);

                if !regression.metrics.is_empty() {
                    regressions.push(regression);
                }
            }
        }

        regressions
    }
}

impl fmt::Display for BenchmarkSuite {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Benchmark Suite ({})", self.timestamp)?;
        writeln!(f, "Results: {}", self.results.len())?;
        for result in &self.results {
            writeln!(f, "\n{}", result)?;
        }
        Ok(())
    }
}

/// Performance regression detection result
#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceRegression {
    pub benchmark_name: String,
    pub workload: String,
    pub metrics: Vec<MetricRegression>,
}

/// Individual metric regression
#[derive(Debug, Serialize, Deserialize)]
pub struct MetricRegression {
    pub metric: String,
    pub baseline_value: f64,
    pub current_value: f64,
    pub change_percent: f64,
    pub is_regression: bool,
}

/// Statistics computed from multiple benchmark results
#[derive(Debug, Default)]
pub struct BenchmarkStatistics {
    pub avg_latency_p99_us: f64,
    pub avg_throughput_ops_per_sec: f64,
    pub avg_write_amp_total: f64,
    pub avg_contention_rate: f64,
    pub max_latency_p99_us: u64,
    pub min_throughput_ops_per_sec: u64,
}

impl BenchmarkStatistics {
    pub fn from_results(results: &[BenchmarkResult]) -> Self {
        if results.is_empty() {
            return Self::default();
        }

        let mut stats = Self {
            avg_latency_p99_us: 0.0,
            avg_throughput_ops_per_sec: 0.0,
            avg_write_amp_total: 0.0,
            avg_contention_rate: 0.0,
            max_latency_p99_us: 0,
            min_throughput_ops_per_sec: u64::MAX,
        };
        let len = results.len() as f64;

        for result in results {
            stats.avg_latency_p99_us += result.latency_p99_us as f64 / len;
            stats.avg_throughput_ops_per_sec += result.throughput_ops_per_sec as f64 / len;
            stats.avg_write_amp_total += result.write_amp_total / len;
            stats.avg_contention_rate += result.lock_contention_rate / len;
            stats.max_latency_p99_us = stats.max_latency_p99_us.max(result.latency_p99_us);
            stats.min_throughput_ops_per_sec = stats.min_throughput_ops_per_sec.min(result.throughput_ops_per_sec);
        }

        stats
    }
}

impl fmt::Display for BenchmarkStatistics {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Benchmark Statistics:\n  \
             Avg P99 Latency: {:.0}μs (max: {}μs)\n  \
             Avg Throughput: {:.0} ops/s (min: {} ops/s)\n  \
             Avg Write Amplification: {:.2}x\n  \
             Avg Lock Contention: {:.1}%",
            self.avg_latency_p99_us,
            self.max_latency_p99_us,
            self.avg_throughput_ops_per_sec,
            self.min_throughput_ops_per_sec,
            self.avg_write_amp_total,
            self.avg_contention_rate * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_result_creation() {
        let result = BenchmarkResult {
            name: "test".into(),
            workload_pattern: "Sequential".into(),
            duration_secs: 1.5,
            latency_min_us: 100,
            latency_max_us: 10000,
            latency_avg_us: 500,
            latency_p50_us: 400,
            latency_p95_us: 2000,
            latency_p99_us: 5000,
            throughput_ops_per_sec: 1000,
            throughput_mb_per_sec: 10.5,
            write_amp_physical: 1.5,
            write_amp_wal: 2.0,
            write_amp_total: 3.5,
            lock_contention_rate: 0.05,
            lock_avg_wait_us: 50,
        };
        assert_eq!(result.name, "test");
        assert_eq!(result.latency_p99_us, 5000);
    }

    #[test]
    fn test_benchmark_suite_statistics() {
        let mut suite = BenchmarkSuite::new();
        suite.add_result(BenchmarkResult {
            name: "test1".into(),
            workload_pattern: "Sequential".into(),
            duration_secs: 1.0,
            latency_min_us: 100,
            latency_max_us: 10000,
            latency_avg_us: 500,
            latency_p50_us: 400,
            latency_p95_us: 2000,
            latency_p99_us: 5000,
            throughput_ops_per_sec: 1000,
            throughput_mb_per_sec: 10.0,
            write_amp_physical: 1.5,
            write_amp_wal: 2.0,
            write_amp_total: 3.5,
            lock_contention_rate: 0.05,
            lock_avg_wait_us: 50,
        });

        let stats = suite.statistics();
        assert_eq!(stats.max_latency_p99_us, 5000);
        assert_eq!(stats.min_throughput_ops_per_sec, 1000);
    }

    #[test]
    fn test_performance_regression_detection() {
        let mut baseline = BenchmarkSuite::new();
        let mut current = BenchmarkSuite::new();

        let baseline_result = BenchmarkResult {
            name: "read_test".into(),
            workload_pattern: "Random".into(),
            duration_secs: 1.0,
            latency_min_us: 100,
            latency_max_us: 5000,
            latency_avg_us: 400,
            latency_p50_us: 300,
            latency_p95_us: 1000,
            latency_p99_us: 2000,
            throughput_ops_per_sec: 10000,
            throughput_mb_per_sec: 100.0,
            write_amp_physical: 1.0,
            write_amp_wal: 1.0,
            write_amp_total: 2.0,
            lock_contention_rate: 0.01,
            lock_avg_wait_us: 10,
        };

        // Simulate 15% regression in P99 latency
        let mut current_result = baseline_result.clone();
        current_result.latency_p99_us = (2000.0 * 1.15) as u64;

        baseline.add_result(baseline_result);
        current.add_result(current_result);

        let regressions = current.compare(&baseline);
        assert!(!regressions.is_empty());
        assert_eq!(regressions[0].benchmark_name, "read_test");
        assert!(regressions[0].metrics.len() > 0);
    }
}
