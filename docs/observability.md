# Observability

Trident logs JSON wide events through `slog`. Production paths should emit one completion event per request, service hop, maintenance job, or benchmark run.

## API

Use the lowercase functions for direct calls:

- `slog::info`
- `slog::warn`
- `slog::warning`
- `slog::error`
- `slog::storage`
- `slog::index`
- `slog::query`
- `slog::accel`
- `slog::compaction`

Use the facade structs when a call site reads better with explicit domains:

- `slog::Info::emit`
- `slog::Warning::emit`
- `slog::Error::emit`
- `slog::Storage::emit`
- `slog::Index::emit`
- `slog::Query::emit`
- `slog::Accel::emit`
- `slog::Compaction::emit`

Use `slog::WideEvent` to collect request context and emit at completion.

## Required Fields

Wide events should include as many of these fields as apply:

- `request_id`
- `operation`
- `model`
- `execution_mode`
- `index_type`
- `record_id`
- `snapshot_id`
- `segment_id`
- `duration_ms`
- `bytes_read`
- `bytes_written`
- `rows_scanned`
- `rows_returned`
- `backend`
- `fallback_used`
- `outcome`
- `error_code`

The logger adds environment fields automatically: `commit`, `version`, `region`, `node`, `service`, `schema_version`, `thread`, and `ts_unix_ms`.

## Rules

- Emit structured JSON fields, not formatted free-text messages.
- Prefer one wide event at completion over scattered logs inside a request.
- Propagate `request_id` through gRPC and internal query execution.
- Use `outcome = "success"` or `outcome = "error"` consistently.
- Include `fallback_used = true` for CPU fallback from GPU paths.
