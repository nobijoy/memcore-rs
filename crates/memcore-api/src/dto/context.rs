use memcore_common::MemcoreError;
use memcore_core::{
    BuildContextOutput, ContextBudget, ContextBudgetUsage, ContextFormat, ContextFormatOptions,
    MemorySearchResult,
};

use super::memories::SearchMemoryFiltersRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

pub fn default_context_max_memories() -> usize {
    memcore_core::DEFAULT_CONTEXT_MAX_MEMORIES
}

pub fn default_context_max_tokens() -> usize {
    memcore_core::DEFAULT_CONTEXT_MAX_TOKENS
}

pub fn default_context_reserved_tokens() -> usize {
    memcore_core::DEFAULT_CONTEXT_RESERVED_TOKENS
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BuildContextRequest {
    pub user_id: String,
    pub query: String,
    #[serde(default = "default_context_max_memories")]
    pub max_memories: usize,
    #[serde(default = "default_context_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_context_reserved_tokens")]
    pub reserved_tokens: usize,
    #[serde(default)]
    pub include_metadata: bool,
    /// Output format: `plain_text`, `markdown`, or `json`.
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub section_by_memory_type: Option<bool>,
    #[serde(default)]
    pub include_memory_ids: Option<bool>,
    #[serde(default)]
    pub include_memory_types: Option<bool>,
    #[serde(default)]
    pub include_scores: Option<bool>,
    #[serde(default)]
    pub include_timestamps: Option<bool>,
    #[serde(default)]
    pub include_confidence: Option<bool>,
    #[serde(default)]
    pub include_importance: Option<bool>,
    #[serde(default)]
    pub filters: SearchMemoryFiltersRequest,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BuildContextResponse {
    pub status: &'static str,
    pub context: String,
    pub memories: Vec<ContextMemoryResponse>,
    pub budget: ContextBudgetResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ContextBudgetResponse {
    pub max_tokens: usize,
    pub reserved_tokens: usize,
    pub available_tokens: usize,
    pub used_tokens: usize,
    pub included_memories: usize,
    pub skipped_memories: usize,
}

impl From<ContextBudgetUsage> for ContextBudgetResponse {
    fn from(usage: ContextBudgetUsage) -> Self {
        Self {
            max_tokens: usage.max_tokens,
            reserved_tokens: usage.reserved_tokens,
            available_tokens: usage.available_tokens,
            used_tokens: usage.used_tokens,
            included_memories: usage.included_memories,
            skipped_memories: usage.skipped_memories,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
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
            budget: output.budget.into(),
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

pub fn format_options_from_request(request: &BuildContextRequest) -> Result<ContextFormatOptions, MemcoreError> {
    let mut options = ContextFormatOptions::default();

    if let Some(format) = &request.format {
        options.format = ContextFormat::parse(format)?;
    }
    if let Some(section_by_memory_type) = request.section_by_memory_type {
        options.section_by_memory_type = section_by_memory_type;
    }
    if let Some(include_memory_ids) = request.include_memory_ids {
        options.include_memory_ids = include_memory_ids;
    }
    if let Some(include_memory_types) = request.include_memory_types {
        options.include_memory_types = include_memory_types;
    }
    if let Some(include_scores) = request.include_scores {
        options.include_scores = include_scores;
    }
    if let Some(include_timestamps) = request.include_timestamps {
        options.include_timestamps = include_timestamps;
    }
    if let Some(include_confidence) = request.include_confidence {
        options.include_confidence = include_confidence;
    }
    if let Some(include_importance) = request.include_importance {
        options.include_importance = include_importance;
    }

    Ok(options)
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

    ContextBudget {
        max_tokens: request.max_tokens,
        reserved_tokens: request.reserved_tokens,
    }
    .validate()?;

    format_options_from_request(request)?;

    Ok(())
}
