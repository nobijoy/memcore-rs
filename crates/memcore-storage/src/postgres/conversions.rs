use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{Fact, MemorySource, MemoryType};
use serde_json::Value;
use uuid::Uuid;

pub(crate) fn memory_type_to_str(value: MemoryType) -> &'static str {
    match value {
        MemoryType::Profile => "profile",
        MemoryType::Preference => "preference",
        MemoryType::Project => "project",
        MemoryType::Conversation => "conversation",
        MemoryType::Task => "task",
        MemoryType::Entity => "entity",
        MemoryType::Skill => "skill",
        MemoryType::System => "system",
    }
}

pub(crate) fn memory_type_from_str(value: &str) -> MemcoreResult<MemoryType> {
    match value {
        "profile" => Ok(MemoryType::Profile),
        "preference" => Ok(MemoryType::Preference),
        "project" => Ok(MemoryType::Project),
        "conversation" => Ok(MemoryType::Conversation),
        "task" => Ok(MemoryType::Task),
        "entity" => Ok(MemoryType::Entity),
        "skill" => Ok(MemoryType::Skill),
        "system" => Ok(MemoryType::System),
        _ => Err(MemcoreError::StorageError(format!(
            "invalid memory_type value: {value}"
        ))),
    }
}

pub(crate) fn memory_source_to_str(value: MemorySource) -> &'static str {
    match value {
        MemorySource::UserMessage => "user_message",
        MemorySource::AssistantMessage => "assistant_message",
        MemorySource::ApiImport => "api_import",
        MemorySource::Manual => "manual",
        MemorySource::System => "system",
    }
}

pub(crate) fn memory_source_from_str(value: &str) -> MemcoreResult<MemorySource> {
    match value {
        "user_message" => Ok(MemorySource::UserMessage),
        "assistant_message" => Ok(MemorySource::AssistantMessage),
        "api_import" => Ok(MemorySource::ApiImport),
        "manual" => Ok(MemorySource::Manual),
        "system" => Ok(MemorySource::System),
        _ => Err(MemcoreError::StorageError(format!(
            "invalid source value: {value}"
        ))),
    }
}

pub(crate) fn row_to_fact(
    id: Uuid,
    org_id: String,
    user_id: String,
    memory_type: String,
    content: String,
    summary: Option<String>,
    source: String,
    confidence: f64,
    importance: f64,
    valid_at: Option<DateTime<Utc>>,
    invalid_at: Option<DateTime<Utc>>,
    recorded_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    metadata: Value,
) -> MemcoreResult<Fact> {
    Ok(Fact {
        id,
        org_id,
        user_id,
        memory_type: memory_type_from_str(&memory_type)?,
        content,
        summary,
        source: memory_source_from_str(&source)?,
        confidence: confidence as f32,
        importance: importance as f32,
        valid_at,
        invalid_at,
        recorded_at,
        updated_at,
        metadata,
    })
}
