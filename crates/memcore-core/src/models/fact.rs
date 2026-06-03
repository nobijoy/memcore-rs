use chrono::{DateTime, Utc};
use memcore_common::MemcoreResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::memory::{FactOperation, MemorySource, MemoryType};
use super::validation::{validate_non_empty, validate_unit_interval};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: Uuid,
    pub org_id: String,
    pub user_id: String,
    pub memory_type: MemoryType,
    pub content: String,
    pub summary: Option<String>,
    pub source: MemorySource,
    pub confidence: f32,
    pub importance: f32,
    pub valid_at: Option<DateTime<Utc>>,
    pub invalid_at: Option<DateTime<Utc>>,
    pub recorded_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: Value,
}

impl Fact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        org_id: impl Into<String>,
        user_id: impl Into<String>,
        memory_type: MemoryType,
        content: impl Into<String>,
        summary: Option<String>,
        source: MemorySource,
        confidence: f32,
        importance: f32,
        valid_at: Option<DateTime<Utc>>,
        invalid_at: Option<DateTime<Utc>>,
        recorded_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        metadata: Value,
    ) -> MemcoreResult<Self> {
        let org_id = org_id.into();
        let user_id = user_id.into();
        let content = content.into();

        validate_non_empty("org_id", &org_id)?;
        validate_non_empty("user_id", &user_id)?;
        validate_non_empty("content", &content)?;
        validate_unit_interval("confidence", confidence)?;
        validate_unit_interval("importance", importance)?;

        Ok(Self {
            id,
            org_id,
            user_id,
            memory_type,
            content,
            summary,
            source,
            confidence,
            importance,
            valid_at,
            invalid_at,
            recorded_at,
            updated_at,
            metadata,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateFact {
    pub content: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
    pub importance: f32,
    pub valid_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

impl CandidateFact {
    pub fn new(
        content: impl Into<String>,
        memory_type: MemoryType,
        confidence: f32,
        importance: f32,
        valid_at: Option<DateTime<Utc>>,
        metadata: Value,
    ) -> MemcoreResult<Self> {
        let content = content.into();
        validate_non_empty("content", &content)?;
        validate_unit_interval("confidence", confidence)?;
        validate_unit_interval("importance", importance)?;

        Ok(Self {
            content,
            memory_type,
            confidence,
            importance,
            valid_at,
            metadata,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactOperationDecision {
    pub operation: FactOperation,
    pub target_fact_id: Option<Uuid>,
    pub reason: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub fact_id: Uuid,
    pub content: String,
    pub memory_type: MemoryType,
    pub score: f32,
    pub confidence: f32,
    pub importance: f32,
    pub valid_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}
