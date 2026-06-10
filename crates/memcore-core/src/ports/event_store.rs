use async_trait::async_trait;
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

#[async_trait]
pub trait MemoryEventStore: Send + Sync {
    async fn record_event(
        &self,
        tenant: &TenantContext,
        event: MemoryEvent,
    ) -> MemcoreResult<MemoryEvent>;

    async fn list_events(&self, query: MemoryEventQuery) -> MemcoreResult<Vec<MemoryEvent>>;
}
