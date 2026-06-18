mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use memcore_api::{AppState, create_app};
use memcore_common::hash_api_key;
use memcore_config::{AuthMode, Settings};
use memcore_core::{
    ApiKeyRecord, ApiKeyScope, ProviderCallStatus, ProviderUsageCapability,
    ProviderUsageEventRecord, ProviderUsageQuery,
};
use tower::ServiceExt;
use uuid::Uuid;

use common::authorization_header;

const ORG_A: &str = "org_jobs_a";
const ORG_B: &str = "org_jobs_b";
const RAW_API_KEY: &str = "jobs-api-key";
const API_KEY_PEPPER: &str = "jobs-pepper";

fn request(method: &str, uri: &str, org_id: Option<&str>, with_auth: bool) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(org_id) = org_id {
        builder = builder.header("X-Organization-ID", org_id);
    }
    if with_auth {
        let (name, value) = authorization_header();
        builder = builder.header(name, value);
    }
    builder.body(Body::empty()).expect("request")
}

fn bearer_request(method: &str, uri: &str, org_id: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Organization-ID", org_id)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request")
}

async fn response_parts(
    app: axum::Router,
    request: Request<Body>,
) -> (StatusCode, serde_json::Value, String) {
    let response = app.oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let text = String::from_utf8_lossy(&body).to_string();
    let json = serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::json!({}));
    (status, json, text)
}

fn jobs_settings() -> Settings {
    let mut settings = Settings::default();
    settings.background_job_org_ids = vec![ORG_A.to_string()];
    settings.memory_usage_snapshot_job_enabled = false;
    settings.provider_usage_retention_job_enabled = false;
    settings
}

fn database_auth_settings() -> Settings {
    let mut settings = Settings::sqlite_memory();
    settings.auth_mode = AuthMode::Database;
    settings.api_key_pepper = Some(API_KEY_PEPPER.to_string());
    settings.dev_api_key = String::new();
    settings.background_job_org_ids = vec![ORG_A.to_string()];
    settings
}

fn api_key_record(org_id: &str, raw_key: &str, scopes: Vec<ApiKeyScope>) -> ApiKeyRecord {
    ApiKeyRecord {
        id: Uuid::new_v4(),
        org_id: org_id.to_string(),
        name: "jobs-key".to_string(),
        key_hash: hash_api_key(API_KEY_PEPPER, raw_key),
        scopes,
        created_at: Utc::now(),
        revoked_at: None,
    }
}

#[tokio::test]
async fn get_jobs_requires_auth_and_org_header() {
    let app = create_app(AppState::new(Settings::default()));

    let (status, json, _) = response_parts(
        app.clone(),
        request("GET", "/api/v1/admin/jobs", Some(ORG_A), false),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"]["code"], "UNAUTHORIZED");

    let (status, json, _) =
        response_parts(app, request("GET", "/api/v1/admin/jobs", None, true)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
}

#[tokio::test]
async fn get_jobs_returns_disabled_defaults() {
    let app = create_app(AppState::new(Settings::default()));
    let (status, json, _) =
        response_parts(app, request("GET", "/api/v1/admin/jobs", Some(ORG_A), true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "success");
    assert_eq!(json["jobs"]["jobs_enabled"], false);
    assert_eq!(json["jobs"]["jobs"].as_array().expect("jobs").len(), 3);
    assert!(
        json["jobs"]["recent_runs"]
            .as_array()
            .expect("runs")
            .is_empty()
    );
}

#[tokio::test]
async fn invalid_job_kind_returns_validation_error() {
    let app = create_app(AppState::new(Settings::default()));
    let (status, json, _) = response_parts(
        app,
        request(
            "POST",
            "/api/v1/admin/jobs/not-a-real-job/run",
            Some(ORG_A),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
}

#[tokio::test]
async fn manual_memory_snapshot_run_works_without_global_runner_enabled() {
    let state = AppState::new(jobs_settings());
    let app = create_app(state);

    let (status, json, body) = response_parts(
        app.clone(),
        request(
            "POST",
            "/api/v1/admin/jobs/memory-usage-snapshot/run",
            Some(ORG_A),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run"]["kind"], "MemoryUsageSnapshot");
    assert_eq!(json["run"]["status"], "Succeeded");
    assert_eq!(json["run"]["affected_count"], 1);
    assert!(!body.contains("secret"));
    assert!(!body.contains("Bearer"));

    let (status, json, _) = response_parts(
        app,
        request(
            "GET",
            "/api/v1/admin/org/usage/memory/snapshots",
            Some(ORG_A),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["snapshots"].as_array().expect("snapshots").len(), 1);
}

#[tokio::test]
async fn manual_provider_usage_retention_run_deletes_only_configured_org() {
    let mut settings = jobs_settings();
    settings.provider_usage_persistence_enabled = true;
    settings.provider_usage_retention_days = 30;
    let state = AppState::new(settings);
    let store = state
        .provider_usage_store
        .clone()
        .expect("mock provider usage store should be configured");

    store
        .record_usage_event(test_usage_event(ORG_A, Utc::now() - Duration::days(45)))
        .await
        .expect("old org_a usage should insert");
    store
        .record_usage_event(test_usage_event(ORG_B, Utc::now() - Duration::days(45)))
        .await
        .expect("old org_b usage should insert");
    store
        .record_usage_event(test_usage_event(ORG_A, Utc::now()))
        .await
        .expect("new org_a usage should insert");

    let app = create_app(state);
    let (status, json, _) = response_parts(
        app,
        request(
            "POST",
            "/api/v1/admin/jobs/provider-usage-retention/run",
            Some(ORG_A),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["run"]["kind"], "ProviderUsageRetention");
    assert_eq!(json["run"]["status"], "Succeeded");
    assert_eq!(json["run"]["affected_count"], 1);

    let org_a = store
        .query_usage(ProviderUsageQuery::new(ORG_A, 10))
        .await
        .expect("org_a usage should query");
    let org_b = store
        .query_usage(ProviderUsageQuery::new(ORG_B, 10))
        .await
        .expect("org_b usage should query");
    assert_eq!(org_a.events.len(), 1);
    assert_eq!(org_b.events.len(), 1);
}

#[tokio::test]
async fn database_auth_scopes_are_enforced_for_job_endpoints() {
    let state = AppState::initialize(database_auth_settings())
        .await
        .expect("state should initialize");
    state
        .api_key_store
        .insert_api_key(api_key_record(
            ORG_A,
            RAW_API_KEY,
            vec![ApiKeyScope::AdminRead],
        ))
        .await
        .expect("api key should insert");
    let app = create_app(state);

    let (status, _, _) = response_parts(
        app.clone(),
        bearer_request("GET", "/api/v1/admin/jobs", ORG_A, RAW_API_KEY),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, json, _) = response_parts(
        app,
        bearer_request(
            "POST",
            "/api/v1/admin/jobs/memory-usage-snapshot/run",
            ORG_A,
            RAW_API_KEY,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"]["code"], "FORBIDDEN");
}

fn test_usage_event(org_id: &str, created_at: chrono::DateTime<Utc>) -> ProviderUsageEventRecord {
    ProviderUsageEventRecord {
        id: Uuid::new_v4(),
        org_id: org_id.to_string(),
        user_id: Some("user_jobs".to_string()),
        provider_name: "mock".to_string(),
        model_name: Some("mock-model".to_string()),
        capability: ProviderUsageCapability::Llm,
        operation_name: "test".to_string(),
        status: ProviderCallStatus::Success,
        input_tokens: Some(1),
        output_tokens: Some(1),
        total_tokens: Some(2),
        retry_count: 0,
        fallback_used: false,
        circuit_blocked: false,
        timed_out: false,
        estimated_cost_usd: None,
        metadata: None,
        created_at,
    }
}
