use axum::Json;
use axum::extract::{Extension, Path, State};
use memcore_common::MemcoreError;
use memcore_core::{ForgetUserInput, TenantContext};

use crate::dto::ForgetUserResponse;
use crate::middleware::OrganizationContext;
use crate::routes::common::ApiError;
use crate::state::AppState;

pub async fn forget_user(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    Path(user_id): Path<String>,
) -> Result<Json<ForgetUserResponse>, ApiError> {
    if user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        )
        .into());
    }

    let tenant = TenantContext::new(organization.org_id, user_id)?;

    let output = state
        .memory_engine
        .forget_user(ForgetUserInput { tenant })
        .await?;

    Ok(Json(ForgetUserResponse::from(output)))
}
