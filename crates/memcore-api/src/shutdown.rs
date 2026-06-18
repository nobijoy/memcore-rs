use memcore_core::ShutdownToken;

pub async fn shutdown_signal() {
    wait_for_shutdown_signal().await;
}

pub async fn shutdown_signal_with_token(token: ShutdownToken) {
    wait_for_shutdown_signal().await;
    tracing::info!("shutdown token cancelled");
    token.cancel();
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(
                error = %error,
                "failed to listen for Ctrl+C shutdown signal"
            );
        }
    };

    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "failed to register SIGTERM shutdown signal"
                    );
                    std::future::pending::<()>().await;
                }
            }
        };

        tokio::select! {
            _ = ctrl_c => {
                tracing::info!(signal = "ctrl_c", "shutdown signal received");
            }
            _ = terminate => {
                tracing::info!(signal = "sigterm", "shutdown signal received");
            }
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
        tracing::info!(signal = "ctrl_c", "shutdown signal received");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_static<T: Send + 'static>(_: T) {}

    #[test]
    fn shutdown_signal_future_can_be_constructed() {
        assert_send_static(shutdown_signal());
    }
}
