use axum::Json;
use axum::extract::{Extension, State};
use memcore_core::BuildContextInput;

use crate::dto::{BuildContextRequest, BuildContextResponse, validate_build_context_request};
use crate::middleware::OrganizationContext;
use crate::routes::common::ApiError;
use crate::state::AppState;

pub async fn build_context(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    Json(request): Json<BuildContextRequest>,
) -> Result<Json<BuildContextResponse>, ApiError> {
    validate_build_context_request(&request)?;

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
        })
        .await?;

    Ok(Json(BuildContextResponse::from(output)))
}
