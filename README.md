# Trident

Trident is the standalone Rust storage engine core for Poiesis. It is not a wrapper over an existing database and it does not carry Poiesis entity semantics inside the core. Trident owns durable value IO, RAM-resident value IO, WAL recovery, immutable segments, cache behavior, snapshots, compaction hooks, and acceleration boundaries.

## Core Shape

- Sync-first engine API with async wrappers.
- WAL-backed atomic write batches.
- MVCC sequence numbers for consistent snapshots.
- RAM memtable plus immutable disk segments.
- Segment block checksums and manifest-driven recovery.
- CPU-complete acceleration interface with optional GPU backends.

## First Commands

```powershell
cargo test
cargo run -- put --data-dir .trident-dev hello world
cargo run -- get --data-dir .trident-dev hello
```
