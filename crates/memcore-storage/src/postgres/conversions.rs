use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{Fact, MemoryEvent, MemoryEventOperation, MemorySource, MemoryType};
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

pub(crate) fn api_key_scope_to_str(value: memcore_core::ApiKeyScope) -> &'static str {
    match value {
        memcore_core::ApiKeyScope::MemoryRead => "memory_read",
        memcore_core::ApiKeyScope::MemoryWrite => "memory_write",
        memcore_core::ApiKeyScope::MemoryDelete => "memory_delete",
        memcore_core::ApiKeyScope::UserDelete => "user_delete",
        memcore_core::ApiKeyScope::AuditRead => "audit_read",
        memcore_core::ApiKeyScope::AdminRead => "admin_read",
        memcore_core::ApiKeyScope::AdminWrite => "admin_write",
    }
}

pub(crate) fn api_key_scope_from_str(value: &str) -> MemcoreResult<memcore_core::ApiKeyScope> {
    match value {
        "memory_read" => Ok(memcore_core::ApiKeyScope::MemoryRead),
        "memory_write" => Ok(memcore_core::ApiKeyScope::MemoryWrite),
        "memory_delete" => Ok(memcore_core::ApiKeyScope::MemoryDelete),
        "user_delete" => Ok(memcore_core::ApiKeyScope::UserDelete),
        "audit_read" => Ok(memcore_core::ApiKeyScope::AuditRead),
        "admin_read" => Ok(memcore_core::ApiKeyScope::AdminRead),
        "admin_write" => Ok(memcore_core::ApiKeyScope::AdminWrite),
        _ => Err(MemcoreError::StorageError(format!(
            "invalid api key scope value: {value}"
        ))),
    }
}

pub(crate) fn memory_event_operation_to_str(value: MemoryEventOperation) -> &'static str {
    match value {
        MemoryEventOperation::Add => "add",
        MemoryEventOperation::Update => "update",
        MemoryEventOperation::Delete => "delete",
        MemoryEventOperation::NoOp => "no_op",
        MemoryEventOperation::ForgetUser => "forget_user",
    }
}

pub(crate) fn memory_event_operation_from_str(value: &str) -> MemcoreResult<MemoryEventOperation> {
    match value {
        "add" => Ok(MemoryEventOperation::Add),
        "update" => Ok(MemoryEventOperation::Update),
        "delete" => Ok(MemoryEventOperation::Delete),
        "no_op" => Ok(MemoryEventOperation::NoOp),
        "forget_user" => Ok(MemoryEventOperation::ForgetUser),
        _ => Err(MemcoreError::StorageError(format!(
            "invalid memory event operation value: {value}"
        ))),
    }
}

pub(crate) fn row_to_memory_event(
    id: Uuid,
    org_id: String,
    user_id: String,
    fact_id: Option<Uuid>,
    operation: String,
    input_text: Option<String>,
    previous_content: Option<String>,
    new_content: Option<String>,
    provider_name: Option<String>,
    model_name: Option<String>,
    metadata: Value,
    created_at: DateTime<Utc>,
) -> MemcoreResult<MemoryEvent> {
    Ok(MemoryEvent {
        id,
        org_id,
        user_id,
        fact_id,
        operation: memory_event_operation_from_str(&operation)?,
        input_text,
        previous_content,
        new_content,
        provider_name,
        model_name,
        metadata,
        created_at,
    })
}
