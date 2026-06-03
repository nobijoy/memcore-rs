use std::net::SocketAddr;

use axum::Router;
use memcore_config::load_settings;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::routes;
use crate::state::AppState;

pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,memcore_api=debug,tower_http=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

pub fn create_app(state: AppState) -> Router {
    routes::router(&state)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();

    let settings = load_settings()?;
    let environment = match settings.environment {
        memcore_config::Environment::Development => "development",
        memcore_config::Environment::Production => "production",
    };
    let storage_mode = match settings.storage_mode {
        memcore_config::StorageMode::Embedded => "embedded",
        memcore_config::StorageMode::Production => "production",
    };

    tracing::info!(
        host = %settings.host,
        port = settings.port,
        environment = environment,
        storage_mode = storage_mode,
        "starting memcore-api"
    );

    let state = AppState::initialize(settings.clone()).await?;
    let app = create_app(state);
    let addr: SocketAddr = format!("{}:{}", settings.host, settings.port).parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "memcore-api listening");
    axum::serve(listener, app).await?;

    Ok(())
}
