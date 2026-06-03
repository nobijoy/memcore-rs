pub mod common;
pub mod health;
pub mod memories;

use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/api/v1/memories", post(memories::add_memory))
        .route("/api/v1/memories/search", post(memories::search_memory))
}
