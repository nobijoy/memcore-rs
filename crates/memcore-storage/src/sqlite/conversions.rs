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

pub(crate) fn datetime_to_str(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

pub(crate) fn datetime_from_str(value: &str) -> MemcoreResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|error| {
            MemcoreError::StorageError(format!("invalid datetime value '{value}': {error}"))
        })
}

pub(crate) fn optional_datetime_to_str(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(datetime_to_str)
}

pub(crate) fn optional_datetime_from_str(value: Option<String>) -> MemcoreResult<Option<DateTime<Utc>>> {
    match value {
        Some(raw) => Ok(Some(datetime_from_str(&raw)?)),
        None => Ok(None),
    }
}

pub(crate) fn metadata_to_str(value: &Value) -> MemcoreResult<String> {
    serde_json::to_string(value).map_err(|error| {
        MemcoreError::StorageError(format!("failed to serialize metadata: {error}"))
    })
}

pub(crate) fn metadata_from_str(value: &str) -> MemcoreResult<Value> {
    serde_json::from_str(value).map_err(|error| {
        MemcoreError::StorageError(format!("failed to deserialize metadata: {error}"))
    })
}

pub(crate) fn row_to_fact(
    id: String,
    org_id: String,
    user_id: String,
    memory_type: String,
    content: String,
    summary: Option<String>,
    source: String,
    confidence: f64,
    importance: f64,
    valid_at: Option<String>,
    invalid_at: Option<String>,
    recorded_at: String,
    updated_at: String,
    metadata: String,
) -> MemcoreResult<Fact> {
    Ok(Fact {
        id: Uuid::parse_str(&id).map_err(|error| {
            MemcoreError::StorageError(format!("invalid fact id '{id}': {error}"))
        })?,
        org_id,
        user_id,
        memory_type: memory_type_from_str(&memory_type)?,
        content,
        summary,
        source: memory_source_from_str(&source)?,
        confidence: confidence as f32,
        importance: importance as f32,
        valid_at: optional_datetime_from_str(valid_at)?,
        invalid_at: optional_datetime_from_str(invalid_at)?,
        recorded_at: datetime_from_str(&recorded_at)?,
        updated_at: datetime_from_str(&updated_at)?,
        metadata: metadata_from_str(&metadata)?,
    })
}
