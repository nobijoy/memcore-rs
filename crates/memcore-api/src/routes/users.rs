use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use memcore_common::MemcoreError;
use memcore_core::{ApiKeyScope, ExportUserDataInput, ForgetUserInput, TenantContext};

use crate::dto::{
    ApplyRetentionRequest, ApplyRetentionResponse, ExportUserQuery, ExportUserResponse,
    ForgetUserResponse, ImportUserDataRequest, ImportUserDataResponse,
};
use crate::middleware::OrganizationContext;
use crate::routes::common::{check_any_scope, check_scope, ApiError};
use crate::security::AuthContext;
use crate::state::AppState;

pub async fn forget_user(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Path(user_id): Path<String>,
) -> Result<Json<ForgetUserResponse>, ApiError> {
    check_scope(auth.as_ref().map(|extension| &extension.0), ApiKeyScope::UserDelete)?;
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

pub async fn export_user_data(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Path(user_id): Path<String>,
    Query(query): Query<ExportUserQuery>,
) -> Result<Json<ExportUserResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[
            ApiKeyScope::AdminRead,
            ApiKeyScope::UserDelete,
            ApiKeyScope::AuditRead,
        ],
    )?;

    if user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        )
        .into());
    }

    let tenant = TenantContext::new(organization.org_id, user_id)?;

    let export = state
        .memory_engine
        .export_user_data(ExportUserDataInput {
            tenant,
            include_events: query.include_events,
            include_deleted: query.include_deleted,
        })
        .await?;

    Ok(Json(ExportUserResponse::from(export)))
}

pub async fn import_user_data(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Path(user_id): Path<String>,
    Json(body): Json<ImportUserDataRequest>,
) -> Result<Json<ImportUserDataResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite, ApiKeyScope::MemoryWrite],
    )?;

    if user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        )
        .into());
    }

    let tenant = TenantContext::new(organization.org_id, user_id)?;

    let output = state
        .memory_engine
        .import_user_data(body.into_input(tenant))
        .await?;

    Ok(Json(ImportUserDataResponse::from(output)))
}

pub async fn apply_retention(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Path(user_id): Path<String>,
    Json(body): Json<ApplyRetentionRequest>,
) -> Result<Json<ApplyRetentionResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite, ApiKeyScope::UserDelete],
    )?;

    if user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        )
        .into());
    }

    let tenant = TenantContext::new(organization.org_id, user_id)?;

    let output = state
        .memory_engine
        .apply_retention(body.into_input(tenant, &state.settings))
        .await?;

    Ok(Json(ApplyRetentionResponse::from(output)))
}
