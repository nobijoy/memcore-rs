use async_trait::async_trait;
use chrono::{DateTime, Utc};
use memcore_common::MemcoreResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::pagination::PageCursor;
use crate::{Fact, MemoryType, TenantContext};

/// Result of a fact retention prune operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPruneResult {
    /// Number of facts matched (dry-run) or soft-deleted (apply).
    pub count: usize,
    /// Fact IDs soft-deleted on apply; empty on dry-run.
    pub fact_ids: Vec<Uuid>,
}

/// Per-user aggregate for organization admin listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgUserSummary {
    pub user_id: String,
    pub memory_count: usize,
    pub last_memory_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactSearchQuery {
    pub tenant: TenantContext,
    pub memory_types: Option<Vec<MemoryType>>,
    pub query_text: Option<String>,
    pub limit: usize,
    pub cursor: Option<PageCursor>,
    pub include_deleted: bool,
}

impl FactSearchQuery {
    pub fn new(tenant: TenantContext, limit: usize) -> Self {
        Self {
            tenant,
            memory_types: None,
            query_text: None,
            limit,
            cursor: None,
            include_deleted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorRecord {
    pub id: Uuid,
    pub fact_id: Uuid,
    pub org_id: String,
    pub user_id: String,
    pub embedding: Vec<f32>,
    pub content: String,
    pub memory_type: MemoryType,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorSearchQuery {
    pub tenant: TenantContext,
    pub embedding: Vec<f32>,
    pub limit: usize,
    pub memory_types: Option<Vec<MemoryType>>,
    pub metadata_filter: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub fact_id: Uuid,
    pub content: String,
    pub score: f32,
    pub memory_type: MemoryType,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgUserListQuery {
    pub org_id: String,
    pub limit: usize,
    pub cursor: Option<PageCursor>,
}

#[async_trait]
pub trait FactStore: Send + Sync {
    async fn insert_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact>;

    async fn update_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact>;

    async fn get_fact(&self, tenant: &TenantContext, fact_id: Uuid) -> MemcoreResult<Option<Fact>>;

    async fn search_facts(&self, query: FactSearchQuery) -> MemcoreResult<Vec<Fact>>;

    async fn soft_delete_fact(&self, tenant: &TenantContext, fact_id: Uuid) -> MemcoreResult<()>;

    async fn delete_user_data(&self, tenant: &TenantContext) -> MemcoreResult<()>;

    /// Soft-deletes active facts with `updated_at` older than `cutoff` for the tenant.
    /// When `dry_run` is true, counts matches without deleting.
    async fn delete_facts_older_than(
        &self,
        tenant: &TenantContext,
        cutoff: DateTime<Utc>,
        dry_run: bool,
    ) -> MemcoreResult<RetentionPruneResult>;

    /// Counts active (non-deleted) facts for an organization.
    async fn count_facts_by_org(&self, org_id: &str) -> MemcoreResult<usize>;

    /// Counts active (non-deleted) facts for one tenant user.
    async fn count_facts_by_user(&self, tenant: &TenantContext) -> MemcoreResult<usize>;

    /// Counts distinct users with at least one active fact in the organization.
    async fn count_users_by_org(&self, org_id: &str) -> MemcoreResult<usize>;

    /// Lists users with memory aggregates for an organization.
    async fn list_users_by_org(
        &self,
        query: OrgUserListQuery,
    ) -> MemcoreResult<Vec<OrgUserSummary>>;
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert_vector(
        &self,
        tenant: &TenantContext,
        record: VectorRecord,
    ) -> MemcoreResult<()>;

    async fn search_vectors(
        &self,
        query: VectorSearchQuery,
    ) -> MemcoreResult<Vec<VectorSearchResult>>;

    async fn delete_by_fact_id(&self, tenant: &TenantContext, fact_id: Uuid) -> MemcoreResult<()>;

    async fn delete_by_user(&self, tenant: &TenantContext) -> MemcoreResult<()>;
}
