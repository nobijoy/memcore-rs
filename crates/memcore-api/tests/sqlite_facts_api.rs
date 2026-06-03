mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use memcore_api::{AppState, create_app};
use memcore_config::Settings;
use tower::ServiceExt;
use uuid::Uuid;

use common::authorization_header;

const ORG_A: &str = "org_sqlite_a";
const ORG_B: &str = "org_sqlite_b";
const USER_A: &str = "user_sqlite_a";
const USER_B: &str = "user_sqlite_b";
const MEMORY_A: &str = "SQLite memory content A";
const MEMORY_B: &str = "SQLite memory content B";

async fn sqlite_app() -> axum::Router {
    let state = AppState::initialize(Settings::sqlite_memory())
        .await
        .expect("sqlite app state should initialize");
    create_app(state)
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

fn get_request(uri: &str, org_id: &str) -> Request<Body> {
    let (auth_name, auth_value) = authorization_header();
    Request::builder()
        .method("GET")
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

async fn seed_two_memories(app: &axum::Router, org_id: &str, user_id: &str) {
    seed_memory(app, org_id, user_id, MEMORY_A).await;
    seed_memory(app, org_id, user_id, MEMORY_B).await;
}

#[tokio::test]
async fn app_starts_with_sqlite_fact_store() {
    let state = AppState::initialize(Settings::sqlite_memory())
        .await
        .expect("sqlite initialization should succeed");
    assert!(matches!(
        state.settings.fact_backend,
        memcore_config::FactBackend::Sqlite
    ));
}

#[tokio::test]
async fn post_memories_inserts_sqlite_backed_facts() {
    let app = sqlite_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_A).await;

    let (status, json) = response_parts(
        app,
        get_request(&format!("/api/v1/users/{USER_A}/memories"), ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["memories"].as_array().unwrap().len(), 1);
    assert_eq!(json["memories"][0]["content"], MEMORY_A);
}

#[tokio::test]
async fn list_memories_reads_sqlite_backed_facts() {
    let app = sqlite_app().await;
    seed_two_memories(&app, ORG_A, USER_A).await;

    let (status, json) = response_parts(
        app,
        get_request(&format!("/api/v1/users/{USER_A}/memories"), ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["memories"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn delete_single_memory_soft_deletes_sqlite_fact() {
    let app = sqlite_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_A).await;

    let (status, list_json) = response_parts(
        app.clone(),
        get_request(&format!("/api/v1/users/{USER_A}/memories"), ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let memory_id = list_json["memories"][0]["id"]
        .as_str()
        .expect("memory id");
    let memory_id = Uuid::parse_str(memory_id).expect("valid uuid");

    let (status, delete_json) = response_parts(
        app.clone(),
        delete_request(
            &format!("/api/v1/users/{USER_A}/memories/{memory_id}"),
            ORG_A,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(delete_json["deleted"], true);

    let (status, list_json) = response_parts(
        app,
        get_request(&format!("/api/v1/users/{USER_A}/memories"), ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list_json["memories"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn forget_user_deletes_sqlite_facts_for_user() {
    let app = sqlite_app().await;
    seed_two_memories(&app, ORG_A, USER_A).await;

    let (status, _) = response_parts(
        app.clone(),
        delete_request(&format!("/api/v1/users/{USER_A}"), ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, json) = response_parts(
        app,
        get_request(&format!("/api/v1/users/{USER_A}/memories"), ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["memories"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn sqlite_tenant_isolation_between_users() {
    let app = sqlite_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_A).await;
    seed_memory(&app, ORG_A, USER_B, MEMORY_B).await;

    let (status, json) = response_parts(
        app,
        get_request(&format!("/api/v1/users/{USER_B}/memories"), ORG_A),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["memories"].as_array().unwrap().len(), 1);
    assert_eq!(json["memories"][0]["content"], MEMORY_B);
}

#[tokio::test]
async fn sqlite_tenant_isolation_between_orgs() {
    let app = sqlite_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_A).await;
    seed_memory(&app, ORG_B, USER_A, MEMORY_B).await;

    let (status, json) = response_parts(
        app,
        get_request(&format!("/api/v1/users/{USER_A}/memories"), ORG_B),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["memories"].as_array().unwrap().len(), 1);
    assert_eq!(json["memories"][0]["content"], MEMORY_B);
}

#[tokio::test]
async fn forgetting_sqlite_user_does_not_delete_other_user_in_same_org() {
    let app = sqlite_app().await;
    seed_memory(&app, ORG_A, USER_A, MEMORY_A).await;
    seed_memory(&app, ORG_A, USER_B, MEMORY_B).await;

    let (status, _) = response_parts(
        app.clone(),
        delete_request(&format!("/api/v1/users/{USER_A}"), ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, json) = response_parts(
        app,
        get_request(&format!("/api/v1/users/{USER_B}/memories"), ORG_A),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["memories"].as_array().unwrap().len(), 1);
}
