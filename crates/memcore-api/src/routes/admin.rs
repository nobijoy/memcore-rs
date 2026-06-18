use axum::Json;
use axum::extract::{Extension, Query, State};
use memcore_common::MemcoreError;
use memcore_core::{
    ApiKeyScope, GetOrgQuotaStatusInput, ListOrgUsersInput, ProviderUsageDailyInput,
    ProviderUsageQuery, SearchOrgMemoryEventsInput, parse_optional_cursor,
    validate_provider_usage_limit,
};
use uuid::Uuid;

use crate::dto::{
    ContextCacheMetricsResponse, CreateMemoryUsageSnapshotRequest,
    CreateMemoryUsageSnapshotResponse, DeleteOrgPlanResponse, GetOrgPlanResponse,
    ListOrgUsersQuery, ListOrgUsersResponse, OrgQuotaStatusResponse, OrgQuotasQuery,
    OrgSummaryResponse, OrgUsageDashboardResponse, OrgUsageDateRangeQuery,
    ProviderUsageDailyQueryParams, ProviderUsageDailyResponse, ProviderUsageQueryParams,
    ProviderUsageResponse, QueryMemoryUsageSnapshotsParams, QueryMemoryUsageSnapshotsResponse,
    SearchOrgMemoryEventsQuery, SearchOrgMemoryEventsResponse, UpsertOrgPlanRequest,
    UpsertOrgPlanResponse, context_cache_metrics_response, get_org_plan_response,
    org_quota_status_response, org_summary_input, org_usage_dashboard_input,
    parse_event_date_filters, parse_keyword_query, parse_memory_event_operation_label,
    parse_org_usage_window, parse_provider_usage_capability, provider_usage_memory_response,
    provider_usage_persisted_response, query_memory_usage_snapshots_input,
    upsert_org_plan_response, validate_list_org_users_limit,
    validate_search_org_memory_events_limit,
};
use crate::middleware::OrganizationContext;
use crate::routes::common::{ApiError, check_any_scope};
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

    let fact_id = query.fact_id.as_deref().map(parse_fact_id).transpose()?;

    let (created_after, created_before) =
        parse_event_date_filters(query.created_after.as_ref(), query.created_before.as_ref())?;
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

pub async fn get_context_cache_metrics(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
) -> Result<Json<ContextCacheMetricsResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let _ = organization.org_id;
    let snapshot = state.memory_engine.context_cache_metrics_snapshot();
    Ok(Json(context_cache_metrics_response(snapshot)))
}

pub async fn get_org_quotas(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<OrgQuotasQuery>,
) -> Result<Json<OrgQuotaStatusResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let result = state
        .memory_engine
        .get_org_quota_status(GetOrgQuotaStatusInput {
            org_id: organization.org_id,
            user_id: query.user_id,
        })
        .await?;

    Ok(Json(org_quota_status_response(result)))
}

pub async fn get_org_plan(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
) -> Result<Json<GetOrgPlanResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let org_id = organization.org_id;
    let plan = state.org_plan_store.get_org_plan(&org_id).await?;
    let resolved = state
        .memory_engine
        .resolve_org_quota_limits(&org_id)
        .await?;

    Ok(Json(get_org_plan_response(plan, resolved)))
}

pub async fn upsert_org_plan(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Json(body): Json<UpsertOrgPlanRequest>,
) -> Result<Json<UpsertOrgPlanResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite],
    )?;

    let org_id = organization.org_id;
    let existing = state.org_plan_store.get_org_plan(&org_id).await?;
    let plan = body.into_plan(org_id, existing.as_ref())?;
    let plan = state.org_plan_store.upsert_org_plan(plan).await?;

    Ok(Json(upsert_org_plan_response(plan)))
}

pub async fn delete_org_plan(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
) -> Result<Json<DeleteOrgPlanResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite],
    )?;

    let deleted = state
        .org_plan_store
        .delete_org_plan(&organization.org_id)
        .await?;

    Ok(Json(DeleteOrgPlanResponse {
        status: "success",
        deleted,
    }))
}

pub async fn get_provider_usage(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<ProviderUsageQueryParams>,
) -> Result<Json<ProviderUsageResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let limit = validate_provider_usage_limit(query.limit)?;
    let capability = query
        .capability
        .as_deref()
        .map(parse_provider_usage_capability)
        .transpose()?;
    let (created_after, created_before) =
        parse_event_date_filters(query.created_after.as_ref(), query.created_before.as_ref())?;
    let cursor = parse_optional_cursor(query.cursor)?;

    let use_persistent = match query.source.as_deref() {
        Some("memory") => false,
        Some("persistent") => true,
        None => state.provider_usage_store.is_some(),
        Some(other) => {
            return Err(MemcoreError::ValidationError(format!("invalid source: {other}")).into());
        }
    };

    if use_persistent {
        if let Some(store) = &state.provider_usage_store {
            let result = store
                .query_usage(ProviderUsageQuery {
                    org_id: organization.org_id.clone(),
                    user_id: query.user_id,
                    provider_name: query.provider_name,
                    model_name: query.model_name,
                    capability,
                    operation_name: query.operation_name,
                    created_after,
                    created_before,
                    limit,
                    cursor,
                })
                .await?;
            return Ok(Json(provider_usage_persisted_response(
                "persistent",
                result,
            )));
        }
    }

    let snapshot = state.provider_usage.snapshot();
    Ok(Json(provider_usage_memory_response(snapshot)))
}

pub async fn get_org_usage_dashboard(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<OrgUsageDateRangeQuery>,
) -> Result<Json<OrgUsageDashboardResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let input = org_usage_dashboard_input(organization.org_id, query)?;
    let output = state.memory_engine.get_org_usage_dashboard(input).await?;

    Ok(Json(OrgUsageDashboardResponse::from(output)))
}

pub async fn create_memory_usage_snapshot(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    body: Option<Json<CreateMemoryUsageSnapshotRequest>>,
) -> Result<Json<CreateMemoryUsageSnapshotResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite],
    )?;

    let body = body
        .map(|Json(body)| body)
        .unwrap_or(CreateMemoryUsageSnapshotRequest { captured_at: None });
    let output = state
        .memory_engine
        .create_memory_usage_snapshot(body.into_input(organization.org_id)?)
        .await?;

    Ok(Json(CreateMemoryUsageSnapshotResponse::from(output)))
}

pub async fn query_memory_usage_snapshots(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<QueryMemoryUsageSnapshotsParams>,
) -> Result<Json<QueryMemoryUsageSnapshotsResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let input = query_memory_usage_snapshots_input(organization.org_id, query)?;
    let output = state
        .memory_engine
        .query_memory_usage_snapshots(input)
        .await?;

    Ok(Json(QueryMemoryUsageSnapshotsResponse::from(output)))
}

pub async fn get_provider_usage_daily(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Query(query): Query<ProviderUsageDailyQueryParams>,
) -> Result<Json<ProviderUsageDailyResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminRead, ApiKeyScope::AdminWrite],
    )?;

    let capability = query
        .capability
        .as_deref()
        .map(parse_provider_usage_capability)
        .transpose()?;
    let (created_after, created_before) = parse_org_usage_window(
        query.created_after.as_ref(),
        query.created_before.as_ref(),
        query.days,
    )?;

    let output = state
        .memory_engine
        .get_provider_usage_daily(ProviderUsageDailyInput {
            org_id: organization.org_id,
            created_after,
            created_before,
            provider_name: query.provider_name,
            model_name: query.model_name,
            capability,
        })
        .await?;

    Ok(Json(ProviderUsageDailyResponse::from(output)))
}

pub async fn apply_provider_usage_retention(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    auth: Option<Extension<AuthContext>>,
    Json(body): Json<crate::dto::ApplyProviderUsageRetentionRequest>,
) -> Result<Json<crate::dto::ApplyProviderUsageRetentionResponse>, ApiError> {
    check_any_scope(
        auth.as_ref().map(|extension| &extension.0),
        &[ApiKeyScope::AdminWrite],
    )?;

    let output = state
        .memory_engine
        .apply_provider_usage_retention(body.into_input(organization.org_id, &state.settings))
        .await?;

    Ok(Json(output.into()))
}

fn parse_fact_id(value: &str) -> Result<Uuid, MemcoreError> {
    Uuid::parse_str(value.trim())
        .map_err(|_| MemcoreError::ValidationError("invalid fact_id".to_string()))
}
