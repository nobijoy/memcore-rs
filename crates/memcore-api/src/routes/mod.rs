pub mod common;
pub mod context;
pub mod health;
pub mod memories;
pub mod memory_events;
pub mod users;

use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{delete, get, post};

use crate::middleware::{require_api_key, require_organization};
use crate::state::AppState;

/// Protected routes: outer `require_api_key` runs first, then `require_organization`, then handler.
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
        .route("/api/v1/users/{user_id}", delete(users::forget_user))
        .route_layer(from_fn(require_organization))
        .route_layer(from_fn_with_state(state.clone(), require_api_key));

    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .merge(protected)
}
