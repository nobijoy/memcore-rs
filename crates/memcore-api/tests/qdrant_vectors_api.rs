//! Qdrant vector backend API integration tests (skipped without MEMCORE_TEST_QDRANT_URL).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use memcore_api::{AppState, create_app};
use memcore_config::{Settings, VectorBackend};
use memcore_core::EMPTY_CONTEXT_MESSAGE;
use tower::ServiceExt;
use uuid::Uuid;

use common::authorization_header;

const ORG_A: &str = "org_qdrant_a";
const USER_A: &str = "user_qdrant_a";
const MEMORY_CONTENT: &str = "Qdrant vector search content for memcore API tests";

fn qdrant_url() -> Option<String> {
    match std::env::var("MEMCORE_TEST_QDRANT_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => None,
    }
}

fn post_request(uri: &str, body: &str, org_id: &str) -> Request<Body> {
    let (auth_name, auth_value) = authorization_header();
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("X-Organization-ID", org_id)
        .header(auth_name, auth_value)
        .body(Body::from(body.to_string()))
        .expect("request should build")
}

async fn response_parts(
    app: axum::Router,
    request: Request<Body>,
) -> (StatusCode, serde_json::Value) {
    let response = app.oneshot(request).await.expect("router should respond");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let json = serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::json!({}));
    (status, json)
}

async fn seed_memory(app: &axum::Router, org_id: &str, user_id: &str, content: &str) {
    let add_body = format!(
        r#"{{
          "user_id": "{user_id}",
          "messages": [{{ "role": "user", "content": "{content}" }}],
          "metadata": {{}}
        }}"#
    );

    let (status, _) = response_parts(
        app.clone(),
        post_request("/api/v1/memories", &add_body, org_id),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn mock_vector_mode_still_works_without_qdrant_server() {
    let state = AppState::new(Settings::default());
    assert_eq!(state.settings.vector_backend, VectorBackend::Mock);
    let app = create_app(state);

    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let search_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}",
          "limit": 5
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["results"].is_array());
}

#[tokio::test]
async fn app_starts_with_qdrant_vector_store_when_server_available() {
    let Some(url) = qdrant_url() else {
        eprintln!("skipping qdrant API test: MEMCORE_TEST_QDRANT_URL not set");
        return;
    };

    let collection = format!("memcore_api_test_{}", Uuid::new_v4().simple());
    let state = AppState::initialize(Settings::qdrant_with_collection(url, collection))
        .await
        .expect("qdrant app state should initialize");

    assert_eq!(state.settings.vector_backend, VectorBackend::Qdrant);
}

#[tokio::test]
async fn search_returns_qdrant_backed_results_after_add_when_server_available() {
    let Some(url) = qdrant_url() else {
        eprintln!("skipping qdrant API test: MEMCORE_TEST_QDRANT_URL not set");
        return;
    };

    let collection = format!("memcore_api_test_{}", Uuid::new_v4().simple());
    let state = AppState::initialize(Settings::qdrant_with_collection(url, collection))
        .await
        .expect("qdrant app state should initialize");
    let app = create_app(state);

    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let search_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}",
          "limit": 5
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let results = json["results"]
        .as_array()
        .expect("search should return results array");
    assert!(!results.is_empty());
}

#[tokio::test]
async fn build_context_works_with_qdrant_when_server_available() {
    let Some(url) = qdrant_url() else {
        eprintln!("skipping qdrant API test: MEMCORE_TEST_QDRANT_URL not set");
        return;
    };

    let collection = format!("memcore_api_test_{}", Uuid::new_v4().simple());
    let state = AppState::initialize(Settings::qdrant_with_collection(url, collection))
        .await
        .expect("qdrant app state should initialize");
    let app = create_app(state);

    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let context_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}",
          "limit": 5
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/context", &context_body, ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let context = json["context"].as_str().expect("context should be a string");
    assert_ne!(context, EMPTY_CONTEXT_MESSAGE);
}
