use memcore_common::MemcoreError;
use memcore_core::{BuildContextOutput, MemorySearchResult};

use super::memories::SearchMemoryFiltersRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn default_context_max_memories() -> usize {
    memcore_core::DEFAULT_CONTEXT_MAX_MEMORIES
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildContextRequest {
    pub user_id: String,
    pub query: String,
    #[serde(default = "default_context_max_memories")]
    pub max_memories: usize,
    #[serde(default)]
    pub include_metadata: bool,
    #[serde(default)]
    pub filters: SearchMemoryFiltersRequest,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildContextResponse {
    pub status: &'static str,
    pub context: String,
    pub memories: Vec<ContextMemoryResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextMemoryResponse {
    pub fact_id: uuid::Uuid,
    pub content: String,
    pub memory_type: super::memories::MemoryTypeResponse,
    pub score: f32,
    pub metadata: Value,
}

impl From<BuildContextOutput> for BuildContextResponse {
    fn from(output: BuildContextOutput) -> Self {
        Self {
            status: "success",
            context: output.context,
            memories: output
                .memories
                .iter()
                .map(ContextMemoryResponse::from)
                .collect(),
        }
    }
}

impl From<&MemorySearchResult> for ContextMemoryResponse {
    fn from(result: &MemorySearchResult) -> Self {
        Self {
            fact_id: result.fact_id,
            content: result.content.clone(),
            memory_type: result.memory_type.into(),
            score: result.score,
            metadata: result.metadata.clone(),
        }
    }
}

pub fn validate_build_context_request(request: &BuildContextRequest) -> Result<(), MemcoreError> {
    if request.user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        ));
    }

    if request.query.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "query cannot be empty".to_string(),
        ));
    }

    if request.max_memories == 0 {
        return Err(MemcoreError::ValidationError(
            "max_memories must be greater than 0".to_string(),
        ));
    }

    if request.max_memories > memcore_core::MAX_CONTEXT_MAX_MEMORIES {
        return Err(MemcoreError::ValidationError(format!(
            "max_memories cannot exceed {}",
            memcore_core::MAX_CONTEXT_MAX_MEMORIES
        )));
    }

    Ok(())
}
