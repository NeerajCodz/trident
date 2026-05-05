# Trident Architecture

Trident is a storage engine in the same family of responsibility as InnoDB, RocksDB, and WiredTiger: it provides the reliable substrate for values, not application-level data modeling.

## Write Path

```text
validate batch
assign sequence number
append WAL record
sync according to policy
apply to memtable
acknowledge
flush immutable state to segment
atomically update manifest
```

## Read Path

```text
memtable
block/value cache
segment metadata
segment file
value log
```

## Acceleration Boundary

Correctness is owned by the scalar CPU implementation. SIMD and GPU backends implement the same `Accelerator` trait and may only change speed, never results.
