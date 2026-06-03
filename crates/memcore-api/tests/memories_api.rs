mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use memcore_api::{AppState, create_app};
use memcore_config::Settings;
use tower::ServiceExt;

use common::authorization_header;

fn test_app() -> axum::Router {
    create_app(AppState::new(Settings::default()))
}

fn add_memory_request(body: &str, org_id: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/memories")
        .header("content-type", "application/json");

    if let Some(org_id) = org_id {
        builder = builder.header("X-Organization-ID", org_id);
    }

    let (name, value) = authorization_header();
    builder = builder.header(name, value);

    builder
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

const VALID_BODY: &str = r#"{
  "user_id": "user_123",
  "messages": [
    {
      "role": "user",
      "content": "I am learning Rust and building a memory engine."
    }
  ],
  "metadata": {
    "session_id": "session_123"
  }
}"#;

#[tokio::test]
async fn post_memories_succeeds_with_valid_request() {
    let (status, json) = response_parts(
        test_app(),
        add_memory_request(VALID_BODY, Some("org_123")),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "success");
}

#[tokio::test]
async fn post_memories_response_includes_operation_summary() {
    let (_, json) = response_parts(
        test_app(),
        add_memory_request(VALID_BODY, Some("org_123")),
    )
    .await;

    assert_eq!(json["summary"]["added"], 1);
    assert_eq!(json["summary"]["updated"], 0);
    assert_eq!(json["summary"]["deleted"], 0);
    assert_eq!(json["summary"]["noop"], 0);
}

#[tokio::test]
async fn post_memories_response_includes_memories() {
    let (_, json) = response_parts(
        test_app(),
        add_memory_request(VALID_BODY, Some("org_123")),
    )
    .await;

    let memories = json["memories"].as_array().expect("memories should be an array");
    assert_eq!(memories.len(), 1);
    assert!(memories[0]["id"].is_string());
    assert_eq!(
        memories[0]["content"],
        "I am learning Rust and building a memory engine."
    );
    assert_eq!(memories[0]["memory_type"], "Conversation");
    assert!(memories[0]["confidence"].is_number());
    assert!(memories[0]["importance"].is_number());
}

#[tokio::test]
async fn missing_organization_header_returns_error() {
    let (status, json) = response_parts(
        test_app(),
        add_memory_request(VALID_BODY, None),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(
        json["error"]["message"],
        "missing X-Organization-ID header"
    );
}

#[tokio::test]
async fn empty_user_id_returns_validation_error() {
    let body = r#"{
      "user_id": "",
      "messages": [{ "role": "user", "content": "hello" }],
      "metadata": {}
    }"#;

    let (status, json) = response_parts(
        test_app(),
        add_memory_request(body, Some("org_123")),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(json["error"]["message"], "user_id cannot be empty");
}

#[tokio::test]
async fn empty_messages_returns_validation_error() {
    let body = r#"{
      "user_id": "user_123",
      "messages": [],
      "metadata": {}
    }"#;

    let (status, json) = response_parts(
        test_app(),
        add_memory_request(body, Some("org_123")),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(json["error"]["message"], "messages cannot be empty");
}

#[tokio::test]
async fn invalid_role_returns_validation_error() {
    let body = r#"{
      "user_id": "user_123",
      "messages": [{ "role": "moderator", "content": "hello" }],
      "metadata": {}
    }"#;

    let (status, json) = response_parts(
        test_app(),
        add_memory_request(body, Some("org_123")),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(json["error"]["message"], "invalid message role: moderator");
}
