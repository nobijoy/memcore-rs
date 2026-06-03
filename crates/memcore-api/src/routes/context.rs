use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use memcore_core::{BuildContextInput, TenantContext};

use crate::dto::{BuildContextRequest, BuildContextResponse, validate_build_context_request};
use crate::routes::common::{ApiError, org_id_from_headers};
use crate::state::AppState;

pub async fn build_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<BuildContextRequest>,
) -> Result<Json<BuildContextResponse>, ApiError> {
    let org_id = org_id_from_headers(&headers)?;
    validate_build_context_request(&request)?;

    let tenant = TenantContext::new(org_id, request.user_id)?;
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
