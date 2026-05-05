use crate::maintenance::JobPriority;
use crate::manifest::ColumnFamilyDescriptor;
use crate::slog;
use crate::{ColumnFamily, Result, TridentConfig, TridentEngine, TridentError, WriteBatch};
use axum::body::Bytes as BodyBytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct RestServerConfig {
    pub engine: TridentConfig,
    pub bind: SocketAddr,
}

#[derive(Clone)]
struct RestState {
    engine: TridentEngine,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: &'static str,
}

pub async fn serve_rest(config: RestServerConfig) -> Result<()> {
    let engine = TridentEngine::open(config.engine)?;
    let state = RestState { engine };
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/kv/{key}", get(get_key).put(put_key).delete(delete_key))
        .route(
            "/v1/cf/{cf}/kv/{key}",
            get(get_cf_key).put(put_cf_key).delete(delete_cf_key),
        )
        .route("/v1/admin/flush", post(flush))
        .route("/v1/admin/compact", post(compact))
        .route("/v1/admin/checkpoint", post(checkpoint))
        .route("/v1/admin/gc", post(gc))
        .route("/v1/admin/stats", get(stats))
        .route("/v1/admin/verify", get(verify))
        .route("/v1/admin/backup", post(backup))
        .route("/v1/admin/restore", post(restore))
        .route("/v1/admin/scan-prefix/{prefix}", get(scan_prefix))
        .route("/v1/admin/maintenance/flush", post(queue_flush))
        .route("/v1/admin/maintenance/compact", post(queue_compact))
        .route("/v1/admin/maintenance/run-next", post(run_next_maintenance))
        .route("/v1/admin/column-families", get(list_column_families))
        .route(
            "/v1/admin/column-families/{name}",
            post(create_column_family).delete(drop_column_family),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|error| TridentError::Server(error.to_string()))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| TridentError::Server(error.to_string()))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn get_key(State(state): State<RestState>, Path(key): Path<String>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.get(key.as_bytes()) {
        Ok(Some(value)) => (StatusCode::OK, value).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => server_error(error),
    };
    log_request("GET", "/v1/kv/{key}", response.status().as_u16(), started);
    response
}

async fn put_key(
    State(state): State<RestState>,
    Path(key): Path<String>,
    body: BodyBytes,
) -> Response {
    let started = std::time::Instant::now();
    let response = match state
        .engine
        .put(BodyBytes::from(key).to_vec(), body.to_vec())
    {
        Ok(sequence) => Json(serde_json::json!({ "sequence": sequence })).into_response(),
        Err(error) => server_error(error),
    };
    log_request("PUT", "/v1/kv/{key}", response.status().as_u16(), started);
    response
}

async fn delete_key(State(state): State<RestState>, Path(key): Path<String>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.delete(BodyBytes::from(key).to_vec()) {
        Ok(sequence) => Json(serde_json::json!({ "sequence": sequence })).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "DELETE",
        "/v1/kv/{key}",
        response.status().as_u16(),
        started,
    );
    response
}

async fn get_cf_key(
    State(state): State<RestState>,
    Path((cf, key)): Path<(String, String)>,
) -> Response {
    let started = std::time::Instant::now();
    let response =
        match state
            .engine
            .get_cf(&ColumnFamily(cf), key.as_bytes(), state.engine.snapshot())
        {
            Ok(Some(value)) => (StatusCode::OK, value).into_response(),
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(error) => server_error(error),
        };
    log_request(
        "GET",
        "/v1/cf/{cf}/kv/{key}",
        response.status().as_u16(),
        started,
    );
    response
}

async fn put_cf_key(
    State(state): State<RestState>,
    Path((cf, key)): Path<(String, String)>,
    body: BodyBytes,
) -> Response {
    let started = std::time::Instant::now();
    let mut batch = WriteBatch::new();
    batch.put(
        ColumnFamily(cf),
        BodyBytes::from(key).to_vec(),
        body.to_vec(),
    );
    let response = match state.engine.write_batch(batch) {
        Ok(sequence) => Json(serde_json::json!({ "sequence": sequence })).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "PUT",
        "/v1/cf/{cf}/kv/{key}",
        response.status().as_u16(),
        started,
    );
    response
}

async fn delete_cf_key(
    State(state): State<RestState>,
    Path((cf, key)): Path<(String, String)>,
) -> Response {
    let started = std::time::Instant::now();
    let mut batch = WriteBatch::new();
    batch.delete(ColumnFamily(cf), BodyBytes::from(key).to_vec());
    let response = match state.engine.write_batch(batch) {
        Ok(sequence) => Json(serde_json::json!({ "sequence": sequence })).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "DELETE",
        "/v1/cf/{cf}/kv/{key}",
        response.status().as_u16(),
        started,
    );
    response
}

async fn flush(State(state): State<RestState>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.flush() {
        Ok(segment_id) => Json(serde_json::json!({ "segment_id": segment_id })).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "POST",
        "/v1/admin/flush",
        response.status().as_u16(),
        started,
    );
    response
}

async fn compact(State(state): State<RestState>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.compact() {
        Ok(segments) => Json(serde_json::json!({ "segments": segments })).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "POST",
        "/v1/admin/compact",
        response.status().as_u16(),
        started,
    );
    response
}

async fn checkpoint(State(state): State<RestState>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.checkpoint() {
        Ok(checkpoint) => Json(checkpoint).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "POST",
        "/v1/admin/checkpoint",
        response.status().as_u16(),
        started,
    );
    response
}

async fn gc(State(state): State<RestState>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.garbage_collect() {
        Ok(report) => Json(report).into_response(),
        Err(error) => server_error(error),
    };
    log_request("POST", "/v1/admin/gc", response.status().as_u16(), started);
    response
}

async fn stats(State(state): State<RestState>) -> Response {
    let started = std::time::Instant::now();
    let response = Json(state.engine.stats()).into_response();
    log_request(
        "GET",
        "/v1/admin/stats",
        response.status().as_u16(),
        started,
    );
    response
}

async fn verify(State(state): State<RestState>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.verify() {
        Ok(report) => Json(report).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "GET",
        "/v1/admin/verify",
        response.status().as_u16(),
        started,
    );
    response
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BackupRequest {
    backup_dir: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RestoreRequest {
    backup_dir: String,
    target_dir: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct QueueJobRequest {
    reason: Option<String>,
    priority: Option<String>,
}

async fn backup(State(state): State<RestState>, Json(request): Json<BackupRequest>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.backup_to(request.backup_dir) {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "POST",
        "/v1/admin/backup",
        response.status().as_u16(),
        started,
    );
    response
}

async fn restore(Json(request): Json<RestoreRequest>) -> Response {
    let started = std::time::Instant::now();
    let response = match TridentEngine::restore_from_backup(request.backup_dir, request.target_dir)
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "POST",
        "/v1/admin/restore",
        response.status().as_u16(),
        started,
    );
    response
}

async fn scan_prefix(State(state): State<RestState>, Path(prefix): Path<String>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.scan_prefix(prefix.as_bytes(), 1000) {
        Ok(rows) => Json(rows).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "GET",
        "/v1/admin/scan-prefix/{prefix}",
        response.status().as_u16(),
        started,
    );
    response
}

async fn queue_flush(
    State(state): State<RestState>,
    Json(request): Json<QueueJobRequest>,
) -> Response {
    let started = std::time::Instant::now();
    let priority = parse_priority(request.priority.as_deref());
    let id = state.engine.enqueue_flush_job(
        request.reason.unwrap_or_else(|| "api".to_string()),
        priority,
    );
    let response = Json(serde_json::json!({ "job_id": id })).into_response();
    log_request(
        "POST",
        "/v1/admin/maintenance/flush",
        response.status().as_u16(),
        started,
    );
    response
}

async fn queue_compact(
    State(state): State<RestState>,
    Json(request): Json<QueueJobRequest>,
) -> Response {
    let started = std::time::Instant::now();
    let priority = parse_priority(request.priority.as_deref());
    let id = state.engine.enqueue_compaction_job(
        state.engine.effective_config().default_compaction_strategy,
        request.reason.unwrap_or_else(|| "api".to_string()),
        priority,
    );
    let response = Json(serde_json::json!({ "job_id": id })).into_response();
    log_request(
        "POST",
        "/v1/admin/maintenance/compact",
        response.status().as_u16(),
        started,
    );
    response
}

async fn run_next_maintenance(State(state): State<RestState>) -> Response {
    let started = std::time::Instant::now();
    let response = match state.engine.run_next_maintenance_job() {
        Ok(job_id) => Json(serde_json::json!({ "job_id": job_id })).into_response(),
        Err(error) => server_error(error),
    };
    log_request(
        "POST",
        "/v1/admin/maintenance/run-next",
        response.status().as_u16(),
        started,
    );
    response
}

async fn list_column_families(State(state): State<RestState>) -> Response {
    Json(state.engine.list_column_families()).into_response()
}

async fn create_column_family(
    State(state): State<RestState>,
    Path(name): Path<String>,
) -> Response {
    match state.engine.create_column_family(ColumnFamilyDescriptor {
        name,
        ..ColumnFamilyDescriptor::default()
    }) {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(error) => server_error(error),
    }
}

async fn drop_column_family(State(state): State<RestState>, Path(name): Path<String>) -> Response {
    match state.engine.drop_column_family(&name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => server_error(error),
    }
}

fn server_error(error: crate::TridentError) -> Response {
    let status = match &error {
        TridentError::UnknownColumnFamily(_) => StatusCode::NOT_FOUND,
        TridentError::ColumnFamilyExists(_) => StatusCode::CONFLICT,
        TridentError::CannotDropDefaultColumnFamily => StatusCode::BAD_REQUEST,
        TridentError::WriteStalled { .. } => StatusCode::TOO_MANY_REQUESTS,
        TridentError::Corrupt { .. } | TridentError::ConfigMismatch(_) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

fn log_request(method: &str, route: &str, status: u16, started: std::time::Instant) {
    let outcome = if status >= 500 {
        "error"
    } else if status >= 400 {
        "client_error"
    } else {
        "success"
    };
    slog::info(
        "rest_request_complete",
        slog::context()
            .with_str("method", method)
            .with_str("route", route)
            .with_u64("status", status as u64)
            .with_u64("duration_ms", started.elapsed().as_millis() as u64)
            .with_str("outcome", outcome),
    );
}

fn parse_priority(priority: Option<&str>) -> JobPriority {
    match priority.unwrap_or("normal") {
        "high" => JobPriority::High,
        "low" => JobPriority::Low,
        _ => JobPriority::Normal,
    }
}
