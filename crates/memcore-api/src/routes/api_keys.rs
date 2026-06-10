use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use chrono::Utc;
use memcore_common::MemcoreError;
use memcore_config::AuthMode;
use memcore_core::{ApiKeyRecord, ApiKeyScope};
use uuid::Uuid;

use crate::dto::{
    parse_create_api_key_request, ApiKeyItemResponse, CreateApiKeyRequest, CreateApiKeyResponse,
    ListApiKeysQuery, ListApiKeysResponse, RevokeApiKeyResponse,
};
use crate::middleware::OrganizationContext;
use crate::routes::common::{check_any_scope, ApiError};
use crate::security::{generate_raw_api_key, hash_api_key_with_pepper, AuthContext};
use crate::state::AppState;

const DEV_MODE_PEPPER: &str = "memcore-dev-pepper";

pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite],
    )?;

    let (name, scopes) = parse_create_api_key_request(request)?;
    let pepper = resolve_api_key_pepper(&state)?;
    let raw_key = generate_raw_api_key();
    let key_hash = hash_api_key_with_pepper(pepper, &raw_key);

    let record = ApiKeyRecord {
        id: Uuid::new_v4(),
        org_id: organization.org_id,
        name,
        key_hash,
        scopes,
        created_at: Utc::now(),
        revoked_at: None,
    };

    let stored = state.api_key_store.insert_api_key(record).await?;

    Ok(Json(CreateApiKeyResponse {
        status: "success",
        api_key: ApiKeyItemResponse::from(&stored),
        raw_key,
    }))
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<ListApiKeysQuery>,
) -> Result<Json<ListApiKeysResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let records = state
        .api_key_store
        .list_api_keys(&organization.org_id, query.include_revoked)
        .await?;

    Ok(Json(ListApiKeysResponse {
        status: "success",
        api_keys: records.iter().map(ApiKeyItemResponse::from).collect(),
    }))
}

pub async fn revoke_api_key(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Path(api_key_id): Path<String>,
) -> Result<Json<RevokeApiKeyResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite],
    )?;

    let key_id = Uuid::parse_str(api_key_id.trim())
        .map_err(|_| MemcoreError::ValidationError("invalid api_key_id".to_string()))?;

    state
        .api_key_store
        .revoke_api_key(&organization.org_id, key_id)
        .await?;

    Ok(Json(RevokeApiKeyResponse {
        status: "success",
        revoked: true,
    }))
}

fn resolve_api_key_pepper(state: &AppState) -> Result<&str, ApiError> {
    if let Some(pepper) = state
        .settings
        .api_key_pepper
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(pepper);
    }

    if state.settings.auth_mode == AuthMode::Dev {
        return Ok(DEV_MODE_PEPPER);
    }

    Err(MemcoreError::Internal(
        "api key pepper is not configured".to_string(),
    )
    .into())
}
