pub mod common;
pub mod context;
pub mod health;
pub mod memories;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};

use crate::middleware::require_api_key;
use crate::state::AppState;

pub fn router(state: &AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/api/v1/memories", post(memories::add_memory))
        .route("/api/v1/memories/search", post(memories::search_memory))
        .route("/api/v1/context", post(context::build_context))
        .route_layer(from_fn_with_state(state.clone(), require_api_key));

    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .merge(protected)
}
