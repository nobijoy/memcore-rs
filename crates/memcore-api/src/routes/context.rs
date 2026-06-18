use std::time::Instant;

use axum::Json;
use axum::extract::{Extension, State};
use memcore_core::{ApiKeyScope, BuildContextInput, ContextBudget};

use crate::dto::{
    BuildContextRequest, BuildContextResponse, compression_options_from_request,
    format_options_from_request, validate_build_context_request,
};
use crate::middleware::OrganizationContext;
use crate::routes::common::{ApiError, check_scope};
use crate::security::AuthContext;
use crate::state::AppState;

pub async fn build_context(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Json(request): Json<BuildContextRequest>,
) -> Result<Json<BuildContextResponse>, ApiError> {
    check_scope(
        auth.as_ref().map(|extension| &extension.0),
        ApiKeyScope::MemoryRead,
    )?;
    validate_build_context_request(&request)?;

    let format_options = format_options_from_request(&request)?;
    let budget = ContextBudget {
        max_tokens: request.max_tokens,
        reserved_tokens: request.reserved_tokens,
    };
    let compression_options =
        compression_options_from_request(&request, budget.available_tokens())?;
    let user_id = request.user_id.clone();
    let tenant = organization.tenant_with_user_id(request.user_id)?;
    let memory_types = request.filters.parse_memory_types()?;

    let build_input = BuildContextInput {
        tenant,
        query: request.query.clone(),
        max_memories: request.max_memories,
        memory_types,
        include_metadata: request.include_metadata,
        budget,
        format_options,
        compression_options,
    };

    let started_at = Instant::now();
    let output = state
        .memory_engine
        .build_context(build_input.clone())
        .await?;
    let duration_ms = started_at.elapsed().as_millis() as u64;

    tracing::debug!(
        event = "context_build_completed",
        org_id = %organization.org_id,
        user_id = %user_id,
        cache_enabled = output.cache.enabled,
        cache_hit = output.cache.hit,
        served_stale = output.cache.served_stale,
        refresh_started = output.cache.refresh_started,
        waited_for_inflight = output.cache.waited_for_inflight,
        duration_ms,
        "context build completed"
    );

    if output.cache.refresh_started {
        let engine = state.memory_engine.clone();
        tokio::spawn(async move {
            let _ = engine.refresh_stale_context(build_input).await;
        });
    }

    Ok(Json(BuildContextResponse::from(output)))
}
