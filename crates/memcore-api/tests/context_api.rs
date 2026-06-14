mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use memcore_api::{AppState, create_app};
use memcore_config::Settings;
use memcore_core::EMPTY_CONTEXT_MESSAGE;
use tower::ServiceExt;

use common::authorization_header;

const ORG_ID: &str = "org_123";
const USER_ID: &str = "user_123";
const MEMORY_CONTENT: &str = "I am learning Rust and building a memory engine.";

fn test_app() -> axum::Router {
    create_app(AppState::new(Settings::default()))
}

fn post_request(uri: &str, body: &str, org_id: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
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

async fn seed_memory(app: &axum::Router) {
    let add_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "messages": [
            {{
              "role": "user",
              "content": "{MEMORY_CONTENT}"
            }}
          ],
          "metadata": {{}}
        }}"#
    );

    let (status, _) = response_parts(
        app.clone(),
        post_request("/api/v1/memories", &add_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn build_context_succeeds_after_adding_memory() {
    let app = test_app();
    seed_memory(&app).await;

    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (status, json) = response_parts(
        app,
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "success");
    assert!(json["context"].as_str().unwrap().contains(MEMORY_CONTENT));
}

#[tokio::test]
async fn build_context_response_includes_formatted_context_string() {
    let app = test_app();
    seed_memory(&app).await;

    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (_, json) = response_parts(
        app,
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    let context = json["context"].as_str().expect("context should be a string");
    assert!(context.starts_with("Relevant long-term memories:"));
    assert!(context.contains(&format!("- {MEMORY_CONTENT}")));
}

#[tokio::test]
async fn build_context_response_includes_memories_array() {
    let app = test_app();
    seed_memory(&app).await;

    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (_, json) = response_parts(
        app,
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    let memories = json["memories"].as_array().expect("memories should be an array");
    assert!(!memories.is_empty());
    assert_eq!(memories[0]["content"], MEMORY_CONTENT);
    assert!(memories[0]["fact_id"].is_string());
    assert!(memories[0]["score"].is_number());
}

#[tokio::test]
async fn missing_organization_header_returns_error() {
    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "Rust"
        }}"#
    );

    let (status, json) = response_parts(
        test_app(),
        post_request("/api/v1/context", &context_body, None),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
}

#[tokio::test]
async fn empty_user_id_returns_validation_error() {
    let context_body = r#"{
      "user_id": "",
      "query": "Rust"
    }"#;

    let (status, json) = response_parts(
        test_app(),
        post_request("/api/v1/context", context_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["message"], "user_id cannot be empty");
}

#[tokio::test]
async fn empty_query_returns_validation_error() {
    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": ""
        }}"#
    );

    let (status, json) = response_parts(
        test_app(),
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["message"], "query cannot be empty");
}

#[tokio::test]
async fn invalid_memory_type_filter_returns_validation_error() {
    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "Rust",
          "filters": {{
            "memory_type": ["InvalidType"]
          }}
        }}"#
    );

    let (status, json) = response_parts(
        test_app(),
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["message"], "invalid memory type: InvalidType");
}

#[tokio::test]
async fn max_memories_defaults_to_ten() {
    let app = test_app();
    seed_memory(&app).await;

    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "{MEMORY_CONTENT}"
        }}"#
    );

    let (status, _) = response_parts(
        app,
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn max_memories_above_max_returns_validation_error() {
    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "Rust",
          "max_memories": 25
        }}"#
    );

    let (status, json) = response_parts(
        test_app(),
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"]["message"], "max_memories cannot exceed 20");
}

#[tokio::test]
async fn no_memories_found_returns_empty_context_message() {
    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "unrelated topic with no stored memories"
        }}"#
    );

    let (status, json) = response_parts(
        test_app(),
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["context"], EMPTY_CONTEXT_MESSAGE);
    assert_eq!(json["memories"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn context_lists_higher_ranked_memory_before_lower_ranked() {
    let app = test_app();
    let first_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "messages": [{{ "role": "user", "content": "First sqlite integration memory alpha bravo charlie delta" }}],
          "metadata": {{}}
        }}"#
    );
    let second_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "messages": [{{ "role": "user", "content": "Second distinct sqlite integration memory foxtrot golf hotel india" }}],
          "metadata": {{}}
        }}"#
    );

    assert_eq!(
        response_parts(
            app.clone(),
            post_request("/api/v1/memories", &first_body, Some(ORG_ID)),
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        response_parts(
            app.clone(),
            post_request("/api/v1/memories", &second_body, Some(ORG_ID)),
        )
        .await
        .0,
        StatusCode::OK
    );

    let context_body = format!(
        r#"{{
          "user_id": "{USER_ID}",
          "query": "integration memory",
          "max_memories": 10
        }}"#
    );

    let (_, json) = response_parts(
        app,
        post_request("/api/v1/context", &context_body, Some(ORG_ID)),
    )
    .await;

    let context = json["context"].as_str().expect("context");
    let memories = json["memories"].as_array().expect("memories");
    assert!(memories.len() >= 2);

    let first_content = memories[0]["content"].as_str().unwrap();
    let first_pos = context.find(first_content).expect("first in context");
    let second_content = memories[1]["content"].as_str().unwrap();
    let second_pos = context.find(second_content).expect("second in context");
    assert!(first_pos < second_pos);
}
