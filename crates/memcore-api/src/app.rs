use std::future::IntoFuture;
use std::net::SocketAddr;

use axum::Router;
use memcore_config::load_settings;
use memcore_core::ShutdownToken;

use crate::observability::{init_logging, log_startup};
use crate::routes;
use crate::shutdown::shutdown_signal_with_token;
use crate::state::AppState;

pub fn create_app(state: AppState) -> Router {
    routes::router(&state).with_state(state)
}

pub async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let settings = load_settings()?;
    init_logging(&settings)?;
    log_startup(&settings);

    let shutdown_token = ShutdownToken::new();
    let state =
        AppState::initialize_with_shutdown(settings.clone(), shutdown_token.child_token()).await?;
    let app = create_app(state);
    let addr: SocketAddr = format!("{}:{}", settings.host, settings.port).parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "memcore-api listening");
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal_with_token(shutdown_token.child_token()));
    let server = server.into_future();
    tokio::pin!(server);

    tokio::select! {
        result = &mut server => {
            result?;
        }
        _ = shutdown_token.cancelled() => {
            tracing::info!(
                timeout_seconds = settings.graceful_shutdown_timeout_seconds,
                "server graceful shutdown started"
            );
            match tokio::time::timeout(
                std::time::Duration::from_secs(settings.graceful_shutdown_timeout_seconds),
                &mut server,
            )
            .await
            {
                Ok(result) => {
                    result?;
                    tracing::info!("server graceful shutdown complete");
                }
                Err(_) => {
                    tracing::warn!(
                        timeout_seconds = settings.graceful_shutdown_timeout_seconds,
                        "server graceful shutdown timeout reached"
                    );
                }
            }
        }
    }

    Ok(())
}
