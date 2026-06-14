use axum::Json;
use axum::extract::{Extension, Query, State};
use memcore_common::MemcoreError;
use memcore_core::{ApiKeyScope, ListOrgUsersInput, SearchOrgMemoryEventsInput};
use uuid::Uuid;

use crate::dto::{
    parse_event_date_filters, parse_memory_event_operation_label, ListOrgUsersQuery,
    ListOrgUsersResponse, OrgSummaryResponse, SearchOrgMemoryEventsQuery,
    SearchOrgMemoryEventsResponse, org_summary_input, parse_keyword_query,
    validate_list_org_users_limit,
    validate_search_org_memory_events_limit,
};
use crate::middleware::OrganizationContext;
use crate::routes::common::{check_any_scope, ApiError};
use crate::security::AuthContext;
use crate::state::AppState;

pub async fn get_org_summary(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
) -> Result<Json<OrgSummaryResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let input = org_summary_input(organization.org_id);
    let output = state.memory_engine.get_org_summary(input).await?;

    Ok(Json(OrgSummaryResponse::from(output)))
}

pub async fn list_org_users(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<ListOrgUsersQuery>,
) -> Result<Json<ListOrgUsersResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    validate_list_org_users_limit(query.limit)?;

    let input = ListOrgUsersInput {
        org_id: organization.org_id,
        limit: query.limit,
        cursor: query.cursor,
    };
    let output = state.memory_engine.list_org_users(input).await?;

    Ok(Json(ListOrgUsersResponse::from(output)))
}

pub async fn search_org_memory_events(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<SearchOrgMemoryEventsQuery>,
) -> Result<Json<SearchOrgMemoryEventsResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[
            ApiKeyScope::AdminRead,
            ApiKeyScope::AdminWrite,
            ApiKeyScope::AuditRead,
        ],
    )?;

    validate_search_org_memory_events_limit(query.limit)?;

    let operation = query
        .operation
        .as_deref()
        .map(parse_memory_event_operation_label)
        .transpose()?;

    let fact_id = query
        .fact_id
        .as_deref()
        .map(parse_fact_id)
        .transpose()?;

    let (created_after, created_before) = parse_event_date_filters(
        query.created_after.as_ref(),
        query.created_before.as_ref(),
    )?;
    let query_text = parse_keyword_query(query.q)?;

    let input = SearchOrgMemoryEventsInput {
        org_id: organization.org_id,
        user_id: query.user_id,
        fact_id,
        operation,
        created_after,
        created_before,
        query_text,
        limit: query.limit,
        cursor: query.cursor,
    };

    let output = state.memory_engine.search_org_memory_events(input).await?;

    Ok(Json(SearchOrgMemoryEventsResponse::from(output)))
}

fn parse_fact_id(value: &str) -> Result<Uuid, MemcoreError> {
    Uuid::parse_str(value.trim())
        .map_err(|_| MemcoreError::ValidationError("invalid fact_id".to_string()))
}
