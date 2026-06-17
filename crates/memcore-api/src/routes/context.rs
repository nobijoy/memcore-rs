use axum::Json;
use axum::extract::{Extension, State};
use memcore_core::{ApiKeyScope, BuildContextInput, ContextBudget};

use crate::dto::{
    compression_options_from_request, format_options_from_request, BuildContextRequest,
    BuildContextResponse, validate_build_context_request,
};
use crate::middleware::OrganizationContext;
use crate::routes::common::{check_scope, ApiError};
use crate::security::AuthContext;
use crate::state::AppState;

pub async fn build_context(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Json(request): Json<BuildContextRequest>,
) -> Result<Json<BuildContextResponse>, ApiError> {
    check_scope(auth.as_ref().map(|extension| &extension.0), ApiKeyScope::MemoryRead)?;
    validate_build_context_request(&request)?;

    let format_options = format_options_from_request(&request)?;
    let budget = ContextBudget {
        max_tokens: request.max_tokens,
        reserved_tokens: request.reserved_tokens,
    };
    let compression_options = compression_options_from_request(&request, budget.available_tokens())?;
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

    let output = state.memory_engine.build_context(build_input.clone()).await?;

    if output.cache.refresh_started {
        let engine = state.memory_engine.clone();
        tokio::spawn(async move {
            let _ = engine.refresh_stale_context(build_input).await;
        });
    }

    Ok(Json(BuildContextResponse::from(output)))
}
