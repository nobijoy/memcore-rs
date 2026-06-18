use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::{OrgPlanConfig, ProviderUsageCapability, QuotaCheckResult};

pub const DEFAULT_ORG_USAGE_DASHBOARD_DAYS: u32 = 30;
pub const MAX_ORG_USAGE_DASHBOARD_DAYS: u32 = 90;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgUsageDashboardInput {
    pub org_id: String,
    pub created_after: DateTime<Utc>,
    pub created_before: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrgUsageDashboardOutput {
    pub org_id: String,
    pub generated_at: DateTime<Utc>,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub plan: Option<OrgPlanConfig>,
    pub quota: QuotaCheckResult,
    pub memory: OrgMemoryUsageSummary,
    pub provider: ProviderUsageDashboardSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgMemoryUsageSummary {
    pub total_users: u64,
    pub total_memories: u64,
    pub active_memories: u64,
    pub deleted_memories: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsageDashboardSummary {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsageDailyBucket {
    pub date: NaiveDate,
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

impl ProviderUsageDailyBucket {
    pub fn empty(date: NaiveDate) -> Self {
        Self {
            date,
            total_requests: 0,
            total_successes: 0,
            total_errors: 0,
            total_retries: 0,
            total_fallbacks: 0,
            total_circuit_blocks: 0,
            total_timeouts: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            total_estimated_cost_usd: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsageDailyInput {
    pub org_id: String,
    pub created_after: DateTime<Utc>,
    pub created_before: DateTime<Utc>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub capability: Option<ProviderUsageCapability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsageDailyOutput {
    pub org_id: String,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub buckets: Vec<ProviderUsageDailyBucket>,
}
