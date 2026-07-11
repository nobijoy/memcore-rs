//! Prometheus metrics foundation for memcore-api.
//!
//! Installs a process-wide Prometheus recorder (idempotent) and exposes scrape
//! rendering via [`PrometheusHandle`]. Metric recording is a no-op when no
//! recorder is installed.

pub mod handlers;
pub mod labels;
pub mod middleware;
pub mod ops;
pub mod provider;
pub mod registry;

pub use handlers::metrics_handler;
pub use labels::normalize_route;
pub use middleware::require_metrics_auth;
pub use provider::PrometheusProviderUsageRecorder;
pub use registry::{
    MetricsExporter, ensure_build_info, install_metrics_recorder, record_auth_failure,
    record_http_request, record_rate_limited,
};
