use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use memcore_config::{Environment, FactBackend, StorageMode, VectorBackend};
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadyResponse {
    pub status: String,
    pub environment: String,
    pub storage_mode: String,
    pub vector_backend: String,
    pub fact_backend: String,
    pub checks: ReadyChecks,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadyChecks {
    pub database: DatabaseReadyCheck,
    pub providers: ProvidersReadyCheck,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DatabaseReadyCheck {
    pub connected: bool,
    pub migrations_clean: bool,
    pub applied_migrations: Option<usize>,
    pub pending_migrations: Option<usize>,
    pub warning_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProvidersReadyCheck {
    pub configured: bool,
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

    let storage = &state.storage_startup;
    let migration_report = storage.migration_report.as_ref();
    let ready = storage.database_connected && storage.migrations_clean;

    Json(ReadyResponse {
        status: if ready { "ready" } else { "not_ready" }.to_string(),
        environment: environment_label(&settings.environment).to_string(),
        storage_mode: storage_mode_label(&settings.storage_mode).to_string(),
        vector_backend: vector_backend_label(&settings.vector_backend).to_string(),
        fact_backend: fact_backend_label(&settings.fact_backend).to_string(),
        checks: ReadyChecks {
            database: DatabaseReadyCheck {
                connected: storage.database_connected,
                migrations_clean: storage.migrations_clean,
                applied_migrations: migration_report.map(|report| report.applied_count),
                pending_migrations: migration_report.map(|report| report.pending_count),
                warning_count: storage.warnings.len(),
            },
            providers: ProvidersReadyCheck { configured: true },
        },
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
