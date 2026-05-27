# Praxis Storage Kernel Verification Report

Date: 2026-05-15

## Summary

The storage kernel builds, passes the active test suite, passes clippy with warnings denied, passes storage boundary checks, compiles all GPU fallback feature sets, and runs graph/vector/search/accel benchmark smoke targets.

The main implementation risk found during stress verification is write-path throughput. The ignored stress suite now supports CI-friendly smoke scale by default and keeps full scale behind `TRIDENT_FULL_STRESS=1`, but even the smoke sequential write workload is slow enough to make write-path optimization the next feature priority.

## Gates Run

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo check --benches --tests` | pass |
| `cargo clippy --all-targets -- -D warnings` | pass |
| `cargo test --all` | pass |
| `scripts/verify_slog_storage.ps1` | pass |
| `scripts/verify_storage_boundary.ps1` | pass |
| `cargo test --test concurrency_benchmark_integration test_phase3b_concurrent_benchmark -- --ignored --exact --test-threads=1` | pass |
| `cargo test --test concurrency_stress -- --ignored --test-threads=1 --nocapture` | pass |

## GPU Feature Matrix

| Feature set | Result |
|---|---|
| `gpu-cuda` | pass |
| `gpu-vulkan` | pass |
| `gpu-metal` | pass |
| `gpu-wgpu` | pass |
| `gpu-cuda,gpu-vulkan,gpu-metal,gpu-wgpu` | pass |

## Stress Results

Ignored stress workloads now default to a smoke scale of 1,000 keys for CI/manual verification. Full stress scale remains available with `TRIDENT_FULL_STRESS=1`, and custom scale remains available with `TRIDENT_STRESS_KEY_COUNT=<n>`.

| Workload | Result | Notes |
|---|---|---|
| sequential writes | pass | 1,000 writes in 36.65s, about 27 ops/sec |
| B-tree large key count | pass | 1,000-key smoke scale |
| LSM large key count | pass | 1,000-key smoke scale |
| hot-key workload | pass | 16,900 accesses in 0.50s |
| uniform workload | pass | 12,309 accesses in 0.50s |

## Benchmark Smoke Results

Criterion settings: `--sample-size 10 --warm-up-time 1 --measurement-time 1`.

| Benchmark | Result |
|---|---|
| `accel_gpu_crc` CUDA fallback | 46.940-48.368 us |
| `accel_gpu_crc` Vulkan fallback | 46.803-54.188 us |
| `accel_gpu_crc` Metal fallback | 49.263-55.118 us |
| `accel_gpu_crc` wgpu fallback | 47.197-60.723 us |
| `graph_adjacency_traversal` one-hop | 87.853-93.061 ns |
| `vector_ivf_search` exact fallback | 49.689-51.992 us |
| `search_inverted_index` term search | 16.295-17.733 us |

## Full Benchmark Note

Running every declared benchmark target in one local loop exceeded one hour and was stopped while `kv_storage` was still active. CI now runs focused smoke benchmarks for acceleration, graph, vector, and search. Full benchmark runs should be split by target or run through scheduled/manual jobs after the write path is optimized.

## Next Feature Priority

1. Optimize durable writes by batching directory/manifest persistence and reducing per-record fsync-style work.
2. Add a benchmark mode for short, full-target CI runs so every `[[bench]]` target can complete under a predictable budget.
3. Add a release-mode full-stress scheduled workflow once write throughput is no longer dominated by per-record persistence overhead.

## Follow-Up Implementation: Batch Write Path

Implemented `StorageEngine::put_batch` and `BatchRecord` so callers can write many canonical values with one segment sync, one WAL batch, one index-application pass, and one record-directory flush. The single-record `put` path remains available and keeps its existing durability behavior.

Post-change stress smoke:

| Workload | Result | Notes |
|---|---|---|
| sequential writes | pass | 1,000 writes in 0.47s, about 2,149 ops/sec |
| B-tree large key count | pass | 1,000-key smoke scale |
| LSM large key count | pass | 1,000-key smoke scale |
| hot-key workload | pass | 17,400 accesses in 0.50s |
| uniform workload | pass | 18,657 accesses in 0.50s |

Write-path improvement on the sequential smoke workload: about 79x faster than the original per-record durability path measured earlier in this report.
