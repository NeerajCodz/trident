# Trident End-to-End Gap Audit TODO (2026-05-08)

## Scope

- Reviewed architecture docs, core engine/store paths, kernel contracts, index implementations, storage formats, recovery/mvcc/tiering modules, API surfaces, and representative test coverage.
- Focus: verify implementation status against the requested storage-kernel direction (invariants, composable engines, durability/recovery core, physical specializations).

## Executive Summary

- The **documentation already contains** most requested sections (`docs/architecture.md`).
- The codebase still has major implementation gaps vs that architecture, especially in runtime invariant enforcement, unified kernel boundaries, memory model, full SSTable/page internals, tiered storage execution, and deterministic failure-recovery rigor.
- There are also **duplicated subsystem trees** that should be unified before scaling complexity.

---

## P0 (Do First)

- [ ] **Convert kernel invariants from doc-only to hard runtime checks + tests**  
  Files: `src/kernel/invariants.rs`, `src/store/engine.rs`, `src/engine/core/engine.rs`, `tests/kv/kernel_invariants_test.rs`  
  Gap: only partial enforcement (mainly pointer-only index layout checks); most invariants are not centrally validated.

- [ ] **Eliminate duplicate engine stacks and pick one canonical storage path**  
  Files: `src/store/*` vs `src/engine/*`, `src/index/*` vs `src/engine/indexes/*`, `src/manifest/*` vs `src/storage/manifest/*`, `src/server/rest.rs` vs `src/api/server/rest.rs`  
  Gap: duplicated implementations risk divergence and make “kernel invariants” unenforceable system-wide.

- [ ] **Make WAL/manifest the single durability authority across all durable artifacts**  
  Files: `src/store/wal.rs`, `src/store/manifest.rs`, `src/store/indirection.rs`, `src/recovery/checkpoint.rs`  
  Gap: several durable files are plain JSON without explicit per-file checksum/version contracts.

- [ ] **Replace write-blocking shared mutex hot paths with read-optimized concurrency model**  
  Files: `src/engine/core/engine.rs`, `src/memory/memtable.rs`, `src/memory/snapshot.rs`  
  Gap: current lock topology does not satisfy “reads must never block writes” for normal paths.

- [ ] **Build deterministic crash/failure simulation suite for the declared failure model**  
  Files: `src/recovery/model.rs`, `src/recovery/recover.rs`, `tests/durability/*`  
  Gap: failure model is modeled as enums/structs, but not fully exercised as deterministic recovery scenarios.

---

## P1 (Architecture-Contract Completion)

- [ ] **Encode “Physical Storage Model” as concrete module boundaries and ownership rules**  
  Files: `src/kernel/physical.rs`, `src/store/mod.rs`, `src/storage/mod.rs`, `src/engine/core/engine.rs`  
  Gap: layering exists in docs, but ownership and interfaces are still mixed/duplicated in code.

- [ ] **Operationalize Storage Engine Laws into guardrails/budgets**  
  Files: `src/engine/core/engine.rs`, `src/maintenance/*`, `src/metrics/*`  
  Gap: some heuristics exist, but no strict policy framework to reject law-violating behavior.

- [ ] **Implement real amplification accounting and SLO checks**  
  Files: `src/store/engine.rs` (`read_amplification`/`space_amplification` placeholders), `src/metrics/*`, `docs/benchmarking.md`  
  Gap: several metrics are static/approximate and not tied to enforceable targets.

- [ ] **Upgrade memory architecture from descriptors to concrete allocators/reclamation**  
  Files: `src/memory/manager.rs`, `src/memory/memtable.rs`, `src/transactions/mvcc.rs`  
  Gap: arena/slab/epoch/RCU concepts are declared but not implemented as core allocation primitives.

- [ ] **Integrate tiered storage policy into live data migration**  
  Files: `src/storage/tiered.rs`, `src/engine/core/engine.rs`, `src/manifest/model.rs`  
  Gap: current tiering is policy-only; no migration/execution pipeline.

- [ ] **Harden manifest/version discipline for every subsystem output**  
  Files: `src/store/engine.rs`, `src/store/manifest.rs`, `src/manifest/model.rs`  
  Gap: not all durable outputs are uniformly tracked with explicit format evolution contracts.

---

## P2 (Physical Engine Depth)

- [ ] **Bring SSTable internals to production shape**  
  Files: `src/storage/lsm/sstable.rs`  
  Gap: has blocks/index/filter/footer/checksums/version, but missing restart intervals, prefix compression, partitioned index/bloom, compression dictionary, explicit range tombstone sections, and zero-copy read path contracts.

- [ ] **Bring B+Tree page layout to full slotted-page design**  
  Files: `src/index/btree/page.rs`, `src/index/btree/manager.rs`  
  Gap: current page model is logical; lacks slot directory/free-space mgmt/overflow chains/ghost records/online defrag/compressed-page lifecycle.

- [ ] **Complete MVCC visibility model in write/read/GC pipelines**  
  Files: `src/transactions/mvcc.rs`, `src/memory/snapshot.rs`, `src/engine/core/engine.rs`  
  Gap: base structs exist; missing full transaction visibility set integration, commit ordering guarantees, and GC horizon enforcement end-to-end.

- [ ] **Complete composable-engine kernel interface (primitives over semantics)**  
  Files: `src/kernel/mod.rs`, `src/store/engine.rs`, `src/query/*`, `src/api/grpc/mod.rs`  
  Gap: kernel primitive direction exists, but query-model/API semantics are still coupled into this repository surface.

---

## Boundary and Non-Goal Hygiene

- [ ] **Enforce “storage-only” boundary in public API surface**  
  Files: `src/query/*`, `src/api/grpc/mod.rs`, `src/server/rest.rs`, `src/api/server/rest.rs`  
  Gap: SQL/graph/vector/search API semantics are represented here; should remain primitive-first to keep Trident as kernel.

- [ ] **Add automated checks to prevent boundary regressions**  
  Files: `tests/integration/*`, `tests/api/*`, `tests/engine/*`  
  Gap: no explicit failing tests for prohibited kernel responsibilities (query planner/parser/coordinator semantics).

---

## Test/Verification Backlog

- [ ] Add invariant conformance tests per subsystem (wal, manifest, value store, each index plugin).
- [ ] Add crash matrix tests for: partial WAL, torn page, manifest interruption, compaction interruption, checksum mismatch.
- [ ] Add deterministic replay tests proving identical recovered state across repeated crash/replay cycles.
- [ ] Add snapshot-visibility property tests during compaction and GC.
- [ ] Add amplification target tests (write/read/space) with threshold assertions.
- [ ] Add workload-tier migration tests (hot/warm/cold/frozen transitions).

---

## Notes from Audit

- `docs/architecture.md` already includes: kernel invariants, storage laws, physical model, performance targets, memory architecture, MVCC model, SSTable/B+tree sections, tiered storage, failure model, non-goals, composable-engine direction.
- Missing TODO/FIXME markers were not found; gaps are mostly **architecture-vs-implementation** gaps rather than explicit code TODO comments.
