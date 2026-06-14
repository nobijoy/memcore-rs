use async_trait::async_trait;
use chrono::{DateTime, Utc};
use memcore_common::MemcoreResult;
use uuid::Uuid;

use crate::{MemoryEvent, MemoryEventOperation, TenantContext};

/// Default limit for listing memory audit events.
pub const DEFAULT_MEMORY_EVENT_LIST_LIMIT: usize = 50;

/// Maximum allowed limit for listing memory audit events.
pub const MAX_MEMORY_EVENT_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEventQuery {
    pub tenant: TenantContext,
    pub fact_id: Option<Uuid>,
    pub operation: Option<MemoryEventOperation>,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl MemoryEventQuery {
    pub fn new(tenant: TenantContext, limit: usize) -> Self {
        Self {
            tenant,
            fact_id: None,
            operation: None,
            limit,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgMemoryEventQuery {
    pub org_id: String,
    pub user_id: Option<String>,
    pub fact_id: Option<Uuid>,
    pub operation: Option<MemoryEventOperation>,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl OrgMemoryEventQuery {
    pub fn new(org_id: String, limit: usize) -> Self {
        Self {
            org_id,
            user_id: None,
            fact_id: None,
            operation: None,
            limit,
            cursor: None,
        }
    }
}

#[async_trait]
pub trait MemoryEventStore: Send + Sync {
    async fn record_event(
        &self,
        tenant: &TenantContext,
        event: MemoryEvent,
    ) -> MemcoreResult<MemoryEvent>;

    async fn list_events(&self, query: MemoryEventQuery) -> MemcoreResult<Vec<MemoryEvent>>;

    /// Lists memory audit events for an organization with optional filters.
    /// Cursor pagination is accepted but not implemented yet.
    async fn list_events_by_org(
        &self,
        query: OrgMemoryEventQuery,
    ) -> MemcoreResult<Vec<MemoryEvent>>;

    /// Hard-deletes memory audit events with `created_at` older than `cutoff` for the tenant.
    /// When `dry_run` is true, counts matches without deleting.
    async fn delete_events_older_than(
        &self,
        tenant: &TenantContext,
        cutoff: DateTime<Utc>,
        dry_run: bool,
    ) -> MemcoreResult<usize>;

    /// Counts memory audit events for an organization.
    async fn count_events_by_org(&self, org_id: &str) -> MemcoreResult<usize>;
}
