use async_trait::async_trait;
use memcore_common::MemcoreResult;
use memcore_core::{Fact, TenantContext};
use uuid::Uuid;

use crate::queries::FactSearchQuery;
use crate::vector::{VectorRecord, VectorSearchQuery, VectorSearchResult};

#[async_trait]
pub trait FactStore: Send + Sync {
    async fn insert_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact>;

    async fn update_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact>;

    async fn get_fact(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<Option<Fact>>;

    async fn search_facts(&self, query: FactSearchQuery) -> MemcoreResult<Vec<Fact>>;

    async fn soft_delete_fact(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<()>;

    async fn delete_user_data(&self, tenant: &TenantContext) -> MemcoreResult<()>;
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

    async fn delete_by_fact_id(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<()>;

    async fn delete_by_user(&self, tenant: &TenantContext) -> MemcoreResult<()>;
}
