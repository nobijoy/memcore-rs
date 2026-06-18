use chrono::{DateTime, Utc};
use memcore_common::MemcoreError;
use memcore_config::Settings;
use memcore_core::{
    ContextCacheMetricsSnapshot, DEFAULT_LIST_ORG_USERS_LIMIT,
    DEFAULT_SEARCH_ORG_MEMORY_EVENTS_LIMIT, ListOrgUsersInput, ListOrgUsersOutput,
    MAX_LIST_ORG_USERS_LIMIT, MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT, MemoryEvent, OrgPlanConfig,
    OrgPlanLimits, OrgPlanTier, OrgQuotaLimits, OrgQuotaUsage, OrgSummaryInput, OrgSummaryOutput,
    OrgUserSummary, QuotaCheckResult, QuotaLimitKind, QuotaViolation, SearchOrgMemoryEventsOutput,
    validate_org_plan_metadata,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::memory_events::MemoryEventOperationResponse;

pub fn default_list_org_users_limit() -> usize {
    DEFAULT_LIST_ORG_USERS_LIMIT
}

pub fn default_search_org_memory_events_limit() -> usize {
    DEFAULT_SEARCH_ORG_MEMORY_EVENTS_LIMIT
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SearchOrgMemoryEventsQuery {
    pub user_id: Option<String>,
    pub fact_id: Option<String>,
    pub operation: Option<String>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub q: Option<String>,
    #[serde(default = "default_search_org_memory_events_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListOrgUsersQuery {
    #[serde(default = "default_list_org_users_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct OrgQuotasQuery {
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchOrgMemoryEventsResponse {
    pub status: &'static str,
    pub events: Vec<AdminOrgMemoryEventItemResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdminOrgMemoryEventItemResponse {
    pub id: Uuid,
    pub user_id: String,
    pub fact_id: Option<Uuid>,
    pub operation: MemoryEventOperationResponse,
    pub previous_content: Option<String>,
    pub new_content: Option<String>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ContextCacheMetricsResponse {
    pub status: &'static str,
    /// Metrics are aggregate counters for this API process only.
    pub scope: &'static str,
    pub metrics: ContextCacheMetricsBodyResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ContextCacheMetricsBodyResponse {
    pub hits: u64,
    pub misses: u64,
    pub sets: u64,
    pub invalidations: u64,
    pub invalidated_entries: u64,
    pub stale_served: u64,
    pub refresh_started: u64,
    pub refresh_succeeded: u64,
    pub refresh_failed: u64,
    pub stampede_waits: u64,
    pub stampede_timeouts: u64,
    pub compute_errors: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgQuotaStatusResponse {
    pub status: &'static str,
    pub quotas: OrgQuotaStatusBodyResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgQuotaStatusBodyResponse {
    pub source: String,
    pub allowed: bool,
    pub limits: OrgQuotaLimitsResponse,
    pub usage: OrgQuotaUsageResponse,
    pub violations: Vec<QuotaViolationResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgQuotaLimitsResponse {
    pub enabled: bool,
    pub max_users_per_org: Option<u64>,
    pub max_memories_per_user: Option<u64>,
    pub max_memories_per_org: Option<u64>,
    pub daily_provider_request_limit: Option<u64>,
    pub daily_provider_token_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgQuotaUsageResponse {
    pub org_id: String,
    pub total_users: u64,
    pub total_memories: u64,
    pub user_memory_count: Option<u64>,
    pub daily_provider_requests: u64,
    pub daily_provider_tokens: u64,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaViolationResponse {
    pub kind: String,
    pub limit: u64,
    pub current: u64,
    pub requested: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgPlanResponse {
    pub org_id: String,
    pub tier: String,
    pub limits: OrgPlanLimitsResponse,
    pub is_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgPlanLimitsResponse {
    pub max_users_per_org: Option<u64>,
    pub max_memories_per_user: Option<u64>,
    pub max_memories_per_org: Option<u64>,
    pub daily_provider_request_limit: Option<u64>,
    pub daily_provider_token_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GetOrgPlanResponse {
    pub status: &'static str,
    pub plan: Option<OrgPlanResponse>,
    pub resolved_source: String,
    pub resolved_limits: OrgQuotaLimitsResponse,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpsertOrgPlanRequest {
    pub tier: String,
    pub limits: UpsertOrgPlanLimitsRequest,
    #[serde(default = "default_true")]
    pub is_active: bool,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpsertOrgPlanLimitsRequest {
    pub max_users_per_org: Option<i64>,
    pub max_memories_per_user: Option<i64>,
    pub max_memories_per_org: Option<i64>,
    pub daily_provider_request_limit: Option<i64>,
    pub daily_provider_token_limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UpsertOrgPlanResponse {
    pub status: &'static str,
    pub plan: OrgPlanResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeleteOrgPlanResponse {
    pub status: &'static str,
    pub deleted: bool,
}

impl From<ContextCacheMetricsSnapshot> for ContextCacheMetricsBodyResponse {
    fn from(snapshot: ContextCacheMetricsSnapshot) -> Self {
        Self {
            hits: snapshot.hits,
            misses: snapshot.misses,
            sets: snapshot.sets,
            invalidations: snapshot.invalidations,
            invalidated_entries: snapshot.invalidated_entries,
            stale_served: snapshot.stale_served,
            refresh_started: snapshot.refresh_started,
            refresh_succeeded: snapshot.refresh_succeeded,
            refresh_failed: snapshot.refresh_failed,
            stampede_waits: snapshot.stampede_waits,
            stampede_timeouts: snapshot.stampede_timeouts,
            compute_errors: snapshot.compute_errors,
        }
    }
}

pub fn context_cache_metrics_response(
    snapshot: ContextCacheMetricsSnapshot,
) -> ContextCacheMetricsResponse {
    ContextCacheMetricsResponse {
        status: "success",
        scope: "process_local",
        metrics: snapshot.into(),
    }
}

pub fn org_quota_limits_from_settings(settings: &Settings) -> OrgQuotaLimits {
    OrgQuotaLimits::from_raw(
        settings.quotas_enabled,
        settings.max_users_per_org,
        settings.max_memories_per_user,
        settings.max_memories_per_org,
        settings.daily_provider_request_limit,
        settings.daily_provider_token_limit,
    )
}

pub fn org_quota_status_response(result: QuotaCheckResult) -> OrgQuotaStatusResponse {
    OrgQuotaStatusResponse {
        status: "success",
        quotas: OrgQuotaStatusBodyResponse {
            source: result.source.as_str().to_string(),
            allowed: result.allowed,
            limits: result.limits.into(),
            usage: result.usage.into(),
            violations: result
                .violations
                .into_iter()
                .map(QuotaViolationResponse::from)
                .collect(),
        },
    }
}

pub fn get_org_plan_response(
    plan: Option<OrgPlanConfig>,
    resolved: memcore_core::ResolvedOrgQuotaLimits,
) -> GetOrgPlanResponse {
    GetOrgPlanResponse {
        status: "success",
        plan: plan.map(OrgPlanResponse::from),
        resolved_source: resolved.source.as_str().to_string(),
        resolved_limits: resolved.limits.into(),
    }
}

pub fn upsert_org_plan_response(plan: OrgPlanConfig) -> UpsertOrgPlanResponse {
    UpsertOrgPlanResponse {
        status: "success",
        plan: plan.into(),
    }
}

impl UpsertOrgPlanRequest {
    pub fn into_plan(
        self,
        org_id: String,
        existing: Option<&OrgPlanConfig>,
    ) -> Result<OrgPlanConfig, MemcoreError> {
        let now = Utc::now();
        let tier = self.tier.parse::<OrgPlanTier>()?;
        validate_org_plan_metadata(self.metadata.as_ref())?;

        let plan = OrgPlanConfig {
            org_id,
            tier,
            limits: self.limits.into_limits()?,
            is_active: self.is_active,
            metadata: self.metadata,
            created_at: existing.map(|plan| plan.created_at).unwrap_or(now),
            updated_at: now,
        };
        plan.validate()?;
        Ok(plan)
    }
}

impl UpsertOrgPlanLimitsRequest {
    fn into_limits(self) -> Result<OrgPlanLimits, MemcoreError> {
        Ok(OrgPlanLimits {
            max_users_per_org: normalize_plan_limit(self.max_users_per_org, "max_users_per_org")?,
            max_memories_per_user: normalize_plan_limit(
                self.max_memories_per_user,
                "max_memories_per_user",
            )?,
            max_memories_per_org: normalize_plan_limit(
                self.max_memories_per_org,
                "max_memories_per_org",
            )?,
            daily_provider_request_limit: normalize_plan_limit(
                self.daily_provider_request_limit,
                "daily_provider_request_limit",
            )?,
            daily_provider_token_limit: normalize_plan_limit(
                self.daily_provider_token_limit,
                "daily_provider_token_limit",
            )?,
        })
    }
}

impl From<OrgPlanConfig> for OrgPlanResponse {
    fn from(plan: OrgPlanConfig) -> Self {
        Self {
            org_id: plan.org_id,
            tier: plan.tier.as_str().to_string(),
            limits: plan.limits.into(),
            is_active: plan.is_active,
            metadata: plan.metadata,
            created_at: plan.created_at,
            updated_at: plan.updated_at,
        }
    }
}

impl From<OrgPlanLimits> for OrgPlanLimitsResponse {
    fn from(limits: OrgPlanLimits) -> Self {
        Self {
            max_users_per_org: limits.max_users_per_org,
            max_memories_per_user: limits.max_memories_per_user,
            max_memories_per_org: limits.max_memories_per_org,
            daily_provider_request_limit: limits.daily_provider_request_limit,
            daily_provider_token_limit: limits.daily_provider_token_limit,
        }
    }
}

fn default_true() -> bool {
    true
}

fn normalize_plan_limit(value: Option<i64>, field: &str) -> Result<Option<u64>, MemcoreError> {
    match value {
        Some(value) if value < 0 => Err(MemcoreError::ValidationError(format!(
            "{field} cannot be negative"
        ))),
        Some(0) | None => Ok(None),
        Some(value) => Ok(Some(value as u64)),
    }
}

impl From<OrgQuotaLimits> for OrgQuotaLimitsResponse {
    fn from(limits: OrgQuotaLimits) -> Self {
        Self {
            enabled: limits.enabled,
            max_users_per_org: limits.max_users_per_org,
            max_memories_per_user: limits.max_memories_per_user,
            max_memories_per_org: limits.max_memories_per_org,
            daily_provider_request_limit: limits.daily_provider_request_limit,
            daily_provider_token_limit: limits.daily_provider_token_limit,
        }
    }
}

impl From<OrgQuotaUsage> for OrgQuotaUsageResponse {
    fn from(usage: OrgQuotaUsage) -> Self {
        Self {
            org_id: usage.org_id,
            total_users: usage.total_users,
            total_memories: usage.total_memories,
            user_memory_count: usage.user_memory_count,
            daily_provider_requests: usage.daily_provider_requests,
            daily_provider_tokens: usage.daily_provider_tokens,
            window_start: usage.window_start,
            window_end: usage.window_end,
        }
    }
}

impl From<QuotaViolation> for QuotaViolationResponse {
    fn from(violation: QuotaViolation) -> Self {
        Self {
            kind: quota_kind_label(violation.kind).to_string(),
            limit: violation.limit,
            current: violation.current,
            requested: violation.requested,
            message: violation.message,
        }
    }
}

fn quota_kind_label(kind: QuotaLimitKind) -> &'static str {
    kind.as_str()
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderUsageResponse {
    pub status: &'static str,
    /// `persistent` reads stored events; `memory` returns process-local aggregates only.
    pub source: String,
    pub summary: ProviderUsageSummaryResponse,
    pub events: Vec<ProviderUsageEventItemResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderUsageSummaryResponse {
    pub total_requests: u64,
    pub total_successes: u64,
    pub total_errors: u64,
    pub total_retries: u64,
    pub total_fallbacks: u64,
    pub total_circuit_blocks: u64,
    pub total_timeouts: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub total_estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderUsageEventItemResponse {
    pub id: Uuid,
    pub provider_name: String,
    pub model_name: Option<String>,
    pub capability: String,
    pub operation_name: String,
    pub status: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub retry_count: u64,
    pub fallback_used: bool,
    pub circuit_blocked: bool,
    pub timed_out: bool,
    pub estimated_cost_usd: Option<f64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProviderUsageQueryParams {
    pub user_id: Option<String>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub capability: Option<String>,
    pub operation_name: Option<String>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    #[serde(default = "default_provider_usage_query_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
    pub source: Option<String>,
}

pub fn default_provider_usage_query_limit() -> usize {
    memcore_core::DEFAULT_PROVIDER_USAGE_LIMIT
}

pub fn parse_provider_usage_capability(
    value: &str,
) -> Result<memcore_core::ProviderUsageCapability, MemcoreError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "llm" => Ok(memcore_core::ProviderUsageCapability::Llm),
        "embedding" => Ok(memcore_core::ProviderUsageCapability::Embedding),
        "summarization" => Ok(memcore_core::ProviderUsageCapability::Summarization),
        _ => Err(MemcoreError::ValidationError(format!(
            "invalid capability: {value}"
        ))),
    }
}

fn capability_label(capability: memcore_core::ProviderUsageCapability) -> String {
    match capability {
        memcore_core::ProviderUsageCapability::Llm => "Llm".to_string(),
        memcore_core::ProviderUsageCapability::Embedding => "Embedding".to_string(),
        memcore_core::ProviderUsageCapability::Summarization => "Summarization".to_string(),
    }
}

fn status_label(status: memcore_core::ProviderCallStatus) -> String {
    match status {
        memcore_core::ProviderCallStatus::Success => "Success".to_string(),
        memcore_core::ProviderCallStatus::Error => "Error".to_string(),
    }
}

impl From<memcore_core::ProviderUsagePersistedSummary> for ProviderUsageSummaryResponse {
    fn from(summary: memcore_core::ProviderUsagePersistedSummary) -> Self {
        Self {
            total_requests: summary.total_requests,
            total_successes: summary.total_successes,
            total_errors: summary.total_errors,
            total_retries: summary.total_retries,
            total_fallbacks: summary.total_fallbacks,
            total_circuit_blocks: summary.total_circuit_blocks,
            total_timeouts: summary.total_timeouts,
            total_input_tokens: summary.total_input_tokens,
            total_output_tokens: summary.total_output_tokens,
            total_tokens: summary.total_tokens,
            total_estimated_cost_usd: summary.total_estimated_cost_usd,
        }
    }
}

pub fn provider_usage_persisted_response(
    source: &str,
    result: memcore_core::ProviderUsageQueryResult,
) -> ProviderUsageResponse {
    ProviderUsageResponse {
        status: "success",
        source: source.to_string(),
        summary: result.summary.into(),
        events: result
            .events
            .into_iter()
            .map(|event| ProviderUsageEventItemResponse {
                id: event.id,
                provider_name: event.provider_name,
                model_name: event.model_name,
                capability: capability_label(event.capability),
                operation_name: event.operation_name,
                status: status_label(event.status),
                input_tokens: event.input_tokens,
                output_tokens: event.output_tokens,
                total_tokens: event.total_tokens,
                retry_count: event.retry_count,
                fallback_used: event.fallback_used,
                circuit_blocked: event.circuit_blocked,
                timed_out: event.timed_out,
                estimated_cost_usd: event.estimated_cost_usd,
                created_at: event.created_at,
            })
            .collect(),
        next_cursor: result.next_cursor,
    }
}

pub fn provider_usage_memory_response(
    snapshot: memcore_providers::ProviderUsageSnapshot,
) -> ProviderUsageResponse {
    let mut total_input_tokens = 0_u64;
    let mut total_output_tokens = 0_u64;
    let mut total_tokens = 0_u64;
    for record in &snapshot.records {
        total_input_tokens = total_input_tokens.saturating_add(record.input_tokens.unwrap_or(0));
        total_output_tokens = total_output_tokens.saturating_add(record.output_tokens.unwrap_or(0));
        total_tokens = total_tokens.saturating_add(record.total_tokens.unwrap_or(0));
    }

    ProviderUsageResponse {
        status: "success",
        source: "memory".to_string(),
        summary: ProviderUsageSummaryResponse {
            total_requests: snapshot.total_requests,
            total_successes: snapshot.total_successes,
            total_errors: snapshot.total_errors,
            total_retries: snapshot.total_retries,
            total_fallbacks: snapshot.total_fallbacks,
            total_circuit_blocks: snapshot.total_circuit_blocks,
            total_timeouts: snapshot.total_timeouts,
            total_input_tokens,
            total_output_tokens,
            total_tokens,
            total_estimated_cost_usd: snapshot.total_estimated_cost_usd,
        },
        events: Vec::new(),
        next_cursor: None,
    }
}

/// Legacy aggregate response shape (process-local counters by provider key).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderUsageLegacyBodyResponse {
    pub total_requests: u64,
    pub total_successes: u64,
    pub total_errors: u64,
    pub total_retries: u64,
    pub total_fallbacks: u64,
    pub total_circuit_blocks: u64,
    pub total_timeouts: u64,
    pub total_estimated_cost_usd: Option<f64>,
    pub records: Vec<ProviderUsageRecordResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderUsageRecordResponse {
    pub provider_name: String,
    pub model_name: Option<String>,
    pub capability: String,
    pub operation_name: String,
    pub request_count: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub retry_count: u64,
    pub fallback_count: u64,
    pub circuit_blocked_count: u64,
    pub timeout_count: u64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgSummaryResponse {
    pub status: &'static str,
    pub summary: OrgSummaryBodyResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgSummaryBodyResponse {
    pub org_id: String,
    pub total_users: usize,
    pub total_facts: usize,
    pub total_events: Option<usize>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ListOrgUsersResponse {
    pub status: &'static str,
    pub users: Vec<OrgUserSummaryResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgUserSummaryResponse {
    pub user_id: String,
    pub memory_count: usize,
    pub last_memory_at: Option<DateTime<Utc>>,
}

impl From<&MemoryEvent> for AdminOrgMemoryEventItemResponse {
    fn from(event: &MemoryEvent) -> Self {
        Self {
            id: event.id,
            user_id: event.user_id.clone(),
            fact_id: event.fact_id,
            operation: event.operation.into(),
            previous_content: event.previous_content.clone(),
            new_content: event.new_content.clone(),
            provider_name: event.provider_name.clone(),
            model_name: event.model_name.clone(),
            metadata: event.metadata.clone(),
            created_at: event.created_at,
        }
    }
}

impl From<OrgSummaryOutput> for OrgSummaryResponse {
    fn from(output: OrgSummaryOutput) -> Self {
        Self {
            status: "success",
            summary: OrgSummaryBodyResponse {
                org_id: output.org_id,
                total_users: output.total_users,
                total_facts: output.total_facts,
                total_events: output.total_events,
            },
        }
    }
}

impl From<OrgUserSummary> for OrgUserSummaryResponse {
    fn from(summary: OrgUserSummary) -> Self {
        Self {
            user_id: summary.user_id,
            memory_count: summary.memory_count,
            last_memory_at: summary.last_memory_at,
        }
    }
}

impl From<ListOrgUsersOutput> for ListOrgUsersResponse {
    fn from(output: ListOrgUsersOutput) -> Self {
        Self {
            status: "success",
            users: output
                .users
                .into_iter()
                .map(OrgUserSummaryResponse::from)
                .collect(),
            next_cursor: output.next_cursor,
        }
    }
}

impl From<SearchOrgMemoryEventsOutput> for SearchOrgMemoryEventsResponse {
    fn from(output: SearchOrgMemoryEventsOutput) -> Self {
        Self {
            status: "success",
            events: output
                .events
                .iter()
                .map(AdminOrgMemoryEventItemResponse::from)
                .collect(),
            next_cursor: output.next_cursor,
        }
    }
}

impl ListOrgUsersQuery {
    pub fn into_input(self, org_id: String) -> ListOrgUsersInput {
        ListOrgUsersInput {
            org_id,
            limit: self.limit,
            cursor: self.cursor,
        }
    }
}

pub fn org_summary_input(org_id: String) -> OrgSummaryInput {
    OrgSummaryInput { org_id }
}

pub fn validate_list_org_users_limit(limit: usize) -> Result<(), memcore_common::MemcoreError> {
    use memcore_common::MemcoreError;

    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > MAX_LIST_ORG_USERS_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_LIST_ORG_USERS_LIMIT}"
        )));
    }

    Ok(())
}

pub fn validate_search_org_memory_events_limit(
    limit: usize,
) -> Result<(), memcore_common::MemcoreError> {
    use memcore_common::MemcoreError;

    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_org_users_limit_defaults_to_fifty() {
        let json = r#"{}"#;
        let query: ListOrgUsersQuery =
            serde_json::from_str(json).expect("deserialize list org users query");
        assert_eq!(query.limit, DEFAULT_LIST_ORG_USERS_LIMIT);
    }

    #[test]
    fn search_org_memory_events_limit_defaults_to_fifty() {
        let json = r#"{}"#;
        let query: SearchOrgMemoryEventsQuery =
            serde_json::from_str(json).expect("deserialize search org memory events query");
        assert_eq!(query.limit, DEFAULT_SEARCH_ORG_MEMORY_EVENTS_LIMIT);
    }
}
