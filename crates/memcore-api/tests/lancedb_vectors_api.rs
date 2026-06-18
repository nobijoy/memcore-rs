mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use memcore_api::{AppState, create_app};
use memcore_config::{Settings, VectorBackend};
use memcore_core::EMPTY_CONTEXT_MESSAGE;
use tempfile::TempDir;
use tower::ServiceExt;
use uuid::Uuid;

use common::authorization_header;

const ORG_A: &str = "org_lance_a";
const ORG_B: &str = "org_lance_b";
const USER_A: &str = "user_lance_a";
const USER_B: &str = "user_lance_b";
const MEMORY_CONTENT: &str = "LanceDB vector search content for memcore tests";

async fn lancedb_app() -> (TempDir, axum::Router) {
    let dir = TempDir::new().expect("temp dir should be created");
    let path = dir.path().to_string_lossy().to_string();
    let state = AppState::initialize(Settings::lancedb_with_path(path))
        .await
        .expect("lancedb app state should initialize");
    (dir, create_app(state))
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

fn delete_request(uri: &str, org_id: &str) -> Request<Body> {
    let (auth_name, auth_value) = authorization_header();
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("X-Organization-ID", org_id)
        .header(auth_name, auth_value)
        .body(Body::empty())
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
async fn app_starts_with_lancedb_vector_store() {
    let dir = TempDir::new().expect("temp dir should be created");
    let state = AppState::initialize(Settings::lancedb_with_path(dir.path().to_string_lossy()))
        .await
        .expect("initialization should succeed");

    assert_eq!(state.settings.vector_backend, VectorBackend::LanceDb);
    drop(state);
}

#[tokio::test]
async fn mock_vector_mode_still_works() {
    let state = AppState::new(Settings::default());
    assert_eq!(state.settings.vector_backend, VectorBackend::Mock);
    let app = create_app(state);

    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let search_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(!json["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn search_returns_lancedb_backed_results_after_add() {
    let (_dir, app) = lancedb_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let search_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let results = json["results"].as_array().expect("results array");
    assert!(!results.is_empty());
    assert_eq!(results[0]["content"], MEMORY_CONTENT);
}

#[tokio::test]
async fn build_context_works_with_lancedb_backed_search() {
    let (_dir, app) = lancedb_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let context_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (status, json) =
        response_parts(app, post_request("/api/v1/context", &context_body, ORG_A)).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["context"].as_str().unwrap().contains(MEMORY_CONTENT));
}

#[tokio::test]
async fn delete_single_memory_removes_lancedb_vector() {
    let (_dir, app) = lancedb_app().await;

    let add_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "messages": [{{ "role": "user", "content": "{MEMORY_CONTENT}" }}],
          "metadata": {{}}
        }}"#
    );
    let (status, add_json) = response_parts(
        app.clone(),
        post_request("/api/v1/memories", &add_body, ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let memory_id = add_json["memories"][0]["id"].as_str().expect("memory id");
    let memory_id = Uuid::parse_str(memory_id).expect("valid uuid");

    let (status, _) = response_parts(
        app.clone(),
        delete_request(
            &format!("/api/v1/users/{USER_A}/memories/{memory_id}"),
            ORG_A,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let search_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );
    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn forget_user_deletes_lancedb_vectors() {
    let (_dir, app) = lancedb_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let (status, _) = response_parts(
        app.clone(),
        delete_request(&format!("/api/v1/users/{USER_A}"), ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let search_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );
    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn lancedb_search_respects_org_isolation() {
    let (_dir, app) = lancedb_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let search_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_B),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn lancedb_search_respects_user_isolation() {
    let (_dir, app) = lancedb_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_CONTENT).await;

    let search_body = format!(
        r#"{{
          "user_id": "{USER_B}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/memories/search", &search_body, ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn lancedb_context_empty_when_no_vectors_for_tenant() {
    let (_dir, app) = lancedb_app().await;

    let context_body = format!(
        r#"{{
          "user_id": "{USER_A}",
          "query": "anything"
        }}"#
    );

    let (status, json) =
        response_parts(app, post_request("/api/v1/context", &context_body, ORG_A)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["context"], EMPTY_CONTEXT_MESSAGE);
}
