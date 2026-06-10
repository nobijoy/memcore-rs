pub mod api_keys;
pub mod common;
pub mod context;
pub mod health;
pub mod memories;
pub mod memory_events;
pub mod users;

use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{delete, get, post};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::middleware::{enforce_rate_limit, require_api_key, require_organization};
use crate::observability::{log_protected_request, observe_request_lifecycle};
use crate::openapi::ApiDoc;
use crate::state::AppState;

/// Protected routes middleware order (incoming request):
/// `observe_request_lifecycle` → `require_api_key` → `require_organization` →
/// `enforce_rate_limit` → `log_protected_request` → handler.
///
/// Axum applies `.route_layer()` in reverse add order on the protected sub-router.
pub fn router(state: &AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/api/v1/memories", post(memories::add_memory))
        .route("/api/v1/memories/search", post(memories::search_memory))
        .route("/api/v1/context", post(context::build_context))
        .route(
            "/api/v1/users/{user_id}/memories",
            get(memories::list_user_memories),
        )
        .route(
            "/api/v1/users/{user_id}/memory-events",
            get(memory_events::list_user_memory_events),
        )
        .route(
            "/api/v1/users/{user_id}/memories/{memory_id}",
            delete(memories::delete_user_memory),
        )
        .route(
            "/api/v1/users/{user_id}/export",
            get(users::export_user_data),
        )
        .route("/api/v1/users/{user_id}", delete(users::forget_user))
        .route("/api/v1/api-keys", post(api_keys::create_api_key))
        .route("/api/v1/api-keys", get(api_keys::list_api_keys))
        .route(
            "/api/v1/api-keys/{api_key_id}",
            delete(api_keys::revoke_api_key),
        )
        .route_layer(from_fn(log_protected_request))
        .route_layer(from_fn_with_state(state.clone(), enforce_rate_limit))
        .route_layer(from_fn(require_organization))
        .route_layer(from_fn_with_state(state.clone(), require_api_key));

    Router::new()
        .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()))
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/metrics", get(health::metrics))
        .merge(protected)
        .layer(from_fn_with_state(state.clone(), observe_request_lifecycle))
}
