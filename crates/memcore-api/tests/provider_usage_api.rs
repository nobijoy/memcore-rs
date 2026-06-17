mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use memcore_api::{
    create_app, create_mock_memory_engine_with_usage, AppState,
};
use memcore_config::Settings;
use memcore_core::{AddMemoryInput, MemoryMessage, MessageRole, TenantContext};
use memcore_providers::InMemoryProviderUsageRecorder;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

use common::authorization_header;

const ORG_ID: &str = "org_provider_usage";
const USER_ID: &str = "user_provider_usage";

fn get_request(path: &str, org_id: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(org_id) = org_id {
        builder = builder.header("X-Organization-ID", org_id);
    }
    let (name, value) = authorization_header();
    builder
        .header(name, value)
        .body(Body::empty())
        .expect("request")
}

#[tokio::test]
async fn provider_usage_requires_authorization() {
    let settings = Settings::default();
    let usage = InMemoryProviderUsageRecorder::new();
    let engine = Arc::new(
        create_mock_memory_engine_with_usage(&settings, usage.clone()).expect("engine"),
    );
    let app = create_app(AppState::with_memory_engine_and_provider_usage(
        settings,
        engine,
        usage,
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/admin/org/provider-usage")
                .header("X-Organization-ID", ORG_ID)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn provider_usage_requires_organization_header() {
    let settings = Settings::default();
    let usage = InMemoryProviderUsageRecorder::new();
    let engine = Arc::new(
        create_mock_memory_engine_with_usage(&settings, usage.clone()).expect("engine"),
    );
    let app = create_app(AppState::with_memory_engine_and_provider_usage(
        settings,
        engine,
        usage,
    ));

    let response = app
        .oneshot(get_request("/api/v1/admin/org/provider-usage", None))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn provider_usage_returns_aggregate_counters() {
    let settings = Settings::default();
    let usage = InMemoryProviderUsageRecorder::new();
    let engine = Arc::new(
        create_mock_memory_engine_with_usage(&settings, usage.clone()).expect("engine"),
    );

    let tenant = TenantContext::new(ORG_ID, USER_ID).expect("tenant");
    let _ = engine
        .add_memory(AddMemoryInput {
            tenant,
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "metrics provider usage test".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory");

    let app = create_app(AppState::with_memory_engine_and_provider_usage(
        settings,
        engine,
        usage,
    ));

    let response = app
        .oneshot(get_request(
            "/api/v1/admin/org/provider-usage",
            Some(ORG_ID),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(json["status"], "success");
    assert_eq!(json["scope"], "process");
    assert!(json["usage"]["total_requests"].as_u64().unwrap_or(0) >= 1);
    assert!(json.get("prompt").is_none());
    assert!(json.get("messages").is_none());
    let body_text = String::from_utf8_lossy(&body);
    assert!(!body_text.contains("metrics provider usage test"));
}
