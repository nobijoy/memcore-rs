use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{delete, get, post};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::metrics::{metrics_handler, require_metrics_auth};
use crate::middleware::{
    apply_security_headers, build_cors_layer, enforce_json_content_type, enforce_rate_limit,
    enforce_request_body_limit, require_api_key, require_organization,
};
use crate::observability::{log_protected_request, observe_request_lifecycle};
use crate::openapi::ApiDoc;
use crate::state::AppState;

pub mod admin;
pub mod api_keys;
pub mod common;
pub mod context;
pub mod health;
pub mod memories;
pub mod memory_events;
pub mod users;

/// Protected routes middleware order (incoming request):
/// `observe_request_lifecycle` → `require_api_key` → `require_organization` →
/// `enforce_rate_limit` → `log_protected_request` → handler.
///
/// Global middleware order (incoming request):
/// request ID / tracing → security headers → CORS (optional) → body limit →
/// content-type validation → routes.
///
/// Axum applies `.layer()` / `.route_layer()` in reverse add order.
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
        .route(
            "/api/v1/users/{user_id}/import",
            post(users::import_user_data),
        )
        .route(
            "/api/v1/users/{user_id}/retention/apply",
            post(users::apply_retention),
        )
        .route("/api/v1/users/{user_id}", delete(users::forget_user))
        .route("/api/v1/api-keys", post(api_keys::create_api_key))
        .route("/api/v1/api-keys", get(api_keys::list_api_keys))
        .route(
            "/api/v1/api-keys/{api_key_id}",
            delete(api_keys::revoke_api_key),
        )
        .route("/api/v1/admin/org/summary", get(admin::get_org_summary))
        .route("/api/v1/admin/jobs", get(admin::get_background_jobs))
        .route(
            "/api/v1/admin/jobs/runs",
            get(admin::query_background_job_runs),
        )
        .route(
            "/api/v1/admin/jobs/runs/retention/apply",
            post(admin::apply_background_job_run_retention),
        )
        .route(
            "/api/v1/admin/jobs/{job_kind}/run",
            post(admin::run_background_job),
        )
        .route("/api/v1/admin/org/users", get(admin::list_org_users))
        .route(
            "/api/v1/admin/org/memory-events",
            get(admin::search_org_memory_events),
        )
        .route(
            "/api/v1/admin/org/cache/context/metrics",
            get(admin::get_context_cache_metrics),
        )
        .route("/api/v1/admin/org/quotas", get(admin::get_org_quotas))
        .route(
            "/api/v1/admin/org/plan",
            get(admin::get_org_plan)
                .put(admin::upsert_org_plan)
                .delete(admin::delete_org_plan),
        )
        .route(
            "/api/v1/admin/org/provider-usage",
            get(admin::get_provider_usage),
        )
        .route(
            "/api/v1/admin/org/usage/dashboard",
            get(admin::get_org_usage_dashboard),
        )
        .route(
            "/api/v1/admin/org/usage/memory/snapshots",
            post(admin::create_memory_usage_snapshot).get(admin::query_memory_usage_snapshots),
        )
        .route(
            "/api/v1/admin/org/usage/provider/daily",
            get(admin::get_provider_usage_daily),
        )
        .route(
            "/api/v1/admin/org/provider-usage/retention/apply",
            post(admin::apply_provider_usage_retention),
        )
        .route_layer(from_fn(log_protected_request))
        .route_layer(from_fn_with_state(state.clone(), enforce_rate_limit))
        .route_layer(from_fn(require_organization))
        .route_layer(from_fn_with_state(state.clone(), require_api_key));

    let metrics_path = state.settings.metrics_path.clone();
    let metrics_router = if state.settings.metrics_enabled {
        let router = Router::new().route(&metrics_path, get(metrics_handler));
        if state.settings.metrics_require_auth {
            router.route_layer(from_fn_with_state(state.clone(), require_metrics_auth))
        } else {
            router
        }
    } else {
        // Keep path registered so disabled scrapes get a stable 404 from the handler.
        Router::new().route(&metrics_path, get(metrics_handler))
    };

    let router = Router::new()
        .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()))
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .merge(metrics_router)
        .route("/api/v1/version", get(health::version))
        .merge(protected)
        .layer(from_fn(enforce_json_content_type))
        .layer(from_fn_with_state(
            state.clone(),
            enforce_request_body_limit,
        ))
        .layer(DefaultBodyLimit::max(state.settings.max_request_body_bytes));

    let router = if let Some(cors_layer) = build_cors_layer(&state.settings) {
        router.layer(cors_layer)
    } else {
        router
    };

    router
        .layer(from_fn_with_state(state.clone(), apply_security_headers))
        .layer(from_fn_with_state(state.clone(), observe_request_lifecycle))
}
