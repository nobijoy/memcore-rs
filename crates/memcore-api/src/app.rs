use std::net::SocketAddr;

use axum::Router;
use memcore_config::load_settings;

use crate::observability::{init_logging, log_startup};
use crate::routes;
use crate::state::AppState;

pub fn create_app(state: AppState) -> Router {
    routes::router(&state).with_state(state)
}

pub async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let settings = load_settings()?;
    init_logging(&settings)?;
    log_startup(&settings);

    let state = AppState::initialize(settings.clone()).await?;
    let app = create_app(state);
    let addr: SocketAddr = format!("{}:{}", settings.host, settings.port).parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "memcore-api listening");
    axum::serve(listener, app).await?;

    Ok(())
}
