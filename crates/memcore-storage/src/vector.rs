use memcore_core::{MemoryType, TenantContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

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
