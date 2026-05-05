use crate::manifest::ColumnFamilyDescriptor;
use crate::{Result, TridentConfig, TridentEngine, TridentError};
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
        .route("/v1/admin/flush", post(flush))
        .route("/v1/admin/compact", post(compact))
        .route("/v1/admin/checkpoint", post(checkpoint))
        .route("/v1/admin/gc", post(gc))
        .route("/v1/admin/stats", get(stats))
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
    match state.engine.get(key.as_bytes()) {
        Ok(Some(value)) => (StatusCode::OK, value).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => server_error(error),
    }
}

async fn put_key(
    State(state): State<RestState>,
    Path(key): Path<String>,
    body: BodyBytes,
) -> Response {
    match state
        .engine
        .put(BodyBytes::from(key).to_vec(), body.to_vec())
    {
        Ok(sequence) => Json(serde_json::json!({ "sequence": sequence })).into_response(),
        Err(error) => server_error(error),
    }
}

async fn delete_key(State(state): State<RestState>, Path(key): Path<String>) -> Response {
    match state.engine.delete(BodyBytes::from(key).to_vec()) {
        Ok(sequence) => Json(serde_json::json!({ "sequence": sequence })).into_response(),
        Err(error) => server_error(error),
    }
}

async fn flush(State(state): State<RestState>) -> Response {
    match state.engine.flush() {
        Ok(segment_id) => Json(serde_json::json!({ "segment_id": segment_id })).into_response(),
        Err(error) => server_error(error),
    }
}

async fn compact(State(state): State<RestState>) -> Response {
    match state.engine.compact() {
        Ok(segments) => Json(serde_json::json!({ "segments": segments })).into_response(),
        Err(error) => server_error(error),
    }
}

async fn checkpoint(State(state): State<RestState>) -> Response {
    match state.engine.checkpoint() {
        Ok(checkpoint) => Json(checkpoint).into_response(),
        Err(error) => server_error(error),
    }
}

async fn gc(State(state): State<RestState>) -> Response {
    match state.engine.garbage_collect() {
        Ok(report) => Json(report).into_response(),
        Err(error) => server_error(error),
    }
}

async fn stats(State(state): State<RestState>) -> Response {
    Json(state.engine.stats()).into_response()
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
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}
