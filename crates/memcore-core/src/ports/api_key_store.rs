use async_trait::async_trait;
use memcore_common::MemcoreResult;
use uuid::Uuid;

use crate::ApiKeyRecord;

#[async_trait]
pub trait ApiKeyStore: Send + Sync {
    async fn find_by_hash(&self, key_hash: &str) -> MemcoreResult<Option<ApiKeyRecord>>;

    async fn insert_api_key(&self, record: ApiKeyRecord) -> MemcoreResult<ApiKeyRecord>;

    async fn revoke_api_key(&self, org_id: &str, key_id: Uuid) -> MemcoreResult<()>;
}
