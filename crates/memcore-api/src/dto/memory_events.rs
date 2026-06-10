use chrono::{DateTime, Utc};
use memcore_common::MemcoreError;
use memcore_core::{
    ListMemoryEventsOutput, MemoryEvent, MemoryEventOperation, DEFAULT_LIST_MEMORY_EVENTS_LIMIT,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

pub fn default_list_memory_events_limit() -> usize {
    DEFAULT_LIST_MEMORY_EVENTS_LIMIT
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListMemoryEventsQuery {
    pub operation: Option<String>,
    pub fact_id: Option<String>,
    #[serde(default = "default_list_memory_events_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ListMemoryEventsResponse {
    pub status: &'static str,
    pub events: Vec<MemoryEventItemResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryEventItemResponse {
    pub id: Uuid,
    pub fact_id: Option<Uuid>,
    pub operation: MemoryEventOperationResponse,
    pub previous_content: Option<String>,
    pub new_content: Option<String>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

/// API-facing operation labels (PascalCase) separate from core snake_case serde.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "PascalCase")]
pub enum MemoryEventOperationResponse {
    Add,
    Update,
    Delete,
    NoOp,
    ForgetUser,
}

impl From<MemoryEventOperation> for MemoryEventOperationResponse {
    fn from(value: MemoryEventOperation) -> Self {
        match value {
            MemoryEventOperation::Add => Self::Add,
            MemoryEventOperation::Update => Self::Update,
            MemoryEventOperation::Delete => Self::Delete,
            MemoryEventOperation::NoOp => Self::NoOp,
            MemoryEventOperation::ForgetUser => Self::ForgetUser,
        }
    }
}

impl From<&MemoryEvent> for MemoryEventItemResponse {
    fn from(event: &MemoryEvent) -> Self {
        Self {
            id: event.id,
            fact_id: event.fact_id,
            operation: event.operation.into(),
            previous_content: event.previous_content.clone(),
            new_content: event.new_content.clone(),
            provider_name: event.provider_name.clone(),
            model_name: event.model_name.clone(),
            metadata: event.metadata.clone(),
            created_at: event.created_at,
        }
    }
}

impl From<ListMemoryEventsOutput> for ListMemoryEventsResponse {
    fn from(output: ListMemoryEventsOutput) -> Self {
        Self {
            status: "success",
            events: output.events.iter().map(MemoryEventItemResponse::from).collect(),
            next_cursor: output.next_cursor,
        }
    }
}

pub fn parse_memory_event_operation_label(label: &str) -> Result<MemoryEventOperation, MemcoreError> {
    match label.trim() {
        "Add" => Ok(MemoryEventOperation::Add),
        "Update" => Ok(MemoryEventOperation::Update),
        "Delete" => Ok(MemoryEventOperation::Delete),
        "NoOp" => Ok(MemoryEventOperation::NoOp),
        "ForgetUser" => Ok(MemoryEventOperation::ForgetUser),
        _ => Err(MemcoreError::ValidationError(
            "invalid operation".to_string(),
        )),
    }
}
