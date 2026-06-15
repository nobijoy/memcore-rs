use axum::Json;
use axum::extract::{Extension, State};
use memcore_core::{ApiKeyScope, BuildContextInput, ContextBudget};

use crate::dto::{
    format_options_from_request, BuildContextRequest, BuildContextResponse,
    validate_build_context_request,
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
    let tenant = organization.tenant_with_user_id(request.user_id)?;
    let memory_types = request.filters.parse_memory_types()?;

    let output = state
        .memory_engine
        .build_context(BuildContextInput {
            tenant,
            query: request.query,
            max_memories: request.max_memories,
            memory_types,
            include_metadata: request.include_metadata,
            budget: ContextBudget {
                max_tokens: request.max_tokens,
                reserved_tokens: request.reserved_tokens,
            },
            format_options,
        })
        .await?;

    Ok(Json(BuildContextResponse::from(output)))
}
