use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use memcore_config::{Environment, FactBackend, StorageMode, VectorBackend};
use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ReadyResponse {
    pub status: &'static str,
    pub environment: String,
    pub storage_mode: String,
    pub vector_backend: String,
    pub fact_backend: String,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "memcore",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub async fn ready(State(state): State<AppState>) -> Json<ReadyResponse> {
    let settings = &state.settings;

    Json(ReadyResponse {
        status: "ready",
        environment: environment_label(&settings.environment).to_string(),
        storage_mode: storage_mode_label(&settings.storage_mode).to_string(),
        vector_backend: vector_backend_label(&settings.vector_backend).to_string(),
        fact_backend: fact_backend_label(&settings.fact_backend).to_string(),
    })
}

/// Minimal Prometheus-compatible metrics (in-process counters only).
pub async fn metrics(State(state): State<AppState>) -> Response {
    if !state.settings.metrics_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(state.metrics.render_prometheus()))
        .expect("metrics response should build")
}

fn environment_label(environment: &Environment) -> &'static str {
    match environment {
        Environment::Development => "development",
        Environment::Production => "production",
    }
}

fn storage_mode_label(storage_mode: &StorageMode) -> &'static str {
    match storage_mode {
        StorageMode::Embedded => "embedded",
        StorageMode::Production => "production",
    }
}

fn vector_backend_label(vector_backend: &VectorBackend) -> &'static str {
    match vector_backend {
        VectorBackend::Mock => "mock",
        VectorBackend::LanceDb => "lancedb",
        VectorBackend::Qdrant => "qdrant",
    }
}

fn fact_backend_label(fact_backend: &FactBackend) -> &'static str {
    match fact_backend {
        FactBackend::Mock => "mock",
        FactBackend::Sqlite => "sqlite",
        FactBackend::Postgres => "postgres",
    }
}
