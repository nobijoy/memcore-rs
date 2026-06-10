use chrono::{DateTime, Utc};
use memcore_common::MemcoreResult;
use memcore_core::{Fact, MemoryEvent, MemorySource, MemoryType, UserMemoryExport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::memories::MemoryTypeResponse;
use super::memory_events::{MemoryEventItemResponse, MemoryEventOperationResponse};

fn default_include_events_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportUserQuery {
    #[serde(default = "default_include_events_true")]
    pub include_events: bool,
    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExportUserResponse {
    pub status: &'static str,
    pub export: UserMemoryExportResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserMemoryExportResponse {
    pub org_id: String,
    pub user_id: String,
    pub exported_at: DateTime<Utc>,
    pub format_version: String,
    pub facts: Vec<ExportFactItemResponse>,
    pub memory_events: Vec<MemoryEventItemResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ExportFactItemResponse {
    pub id: Uuid,
    pub org_id: String,
    pub user_id: String,
    pub content: String,
    pub summary: Option<String>,
    pub memory_type: MemoryTypeResponse,
    pub source: ExportMemorySourceResponse,
    pub confidence: f32,
    pub importance: f32,
    pub valid_at: Option<DateTime<Utc>>,
    pub invalid_at: Option<DateTime<Utc>>,
    pub recorded_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportMemorySourceResponse {
    UserMessage,
    AssistantMessage,
    Manual,
    ApiImport,
    System,
}

impl From<MemorySource> for ExportMemorySourceResponse {
    fn from(value: MemorySource) -> Self {
        match value {
            MemorySource::UserMessage => Self::UserMessage,
            MemorySource::AssistantMessage => Self::AssistantMessage,
            MemorySource::Manual => Self::Manual,
            MemorySource::ApiImport => Self::ApiImport,
            MemorySource::System => Self::System,
        }
    }
}

impl From<&Fact> for ExportFactItemResponse {
    fn from(fact: &Fact) -> Self {
        Self {
            id: fact.id,
            org_id: fact.org_id.clone(),
            user_id: fact.user_id.clone(),
            content: fact.content.clone(),
            summary: fact.summary.clone(),
            memory_type: fact.memory_type.into(),
            source: fact.source.into(),
            confidence: fact.confidence,
            importance: fact.importance,
            valid_at: fact.valid_at,
            invalid_at: fact.invalid_at,
            recorded_at: fact.recorded_at,
            updated_at: fact.updated_at,
            metadata: fact.metadata.clone(),
        }
    }
}

impl From<MemoryTypeResponse> for MemoryType {
    fn from(value: MemoryTypeResponse) -> Self {
        match value {
            MemoryTypeResponse::Profile => Self::Profile,
            MemoryTypeResponse::Preference => Self::Preference,
            MemoryTypeResponse::Project => Self::Project,
            MemoryTypeResponse::Conversation => Self::Conversation,
            MemoryTypeResponse::Task => Self::Task,
            MemoryTypeResponse::Entity => Self::Entity,
            MemoryTypeResponse::Skill => Self::Skill,
            MemoryTypeResponse::System => Self::System,
        }
    }
}

impl From<ExportMemorySourceResponse> for MemorySource {
    fn from(value: ExportMemorySourceResponse) -> Self {
        match value {
            ExportMemorySourceResponse::UserMessage => Self::UserMessage,
            ExportMemorySourceResponse::AssistantMessage => Self::AssistantMessage,
            ExportMemorySourceResponse::Manual => Self::Manual,
            ExportMemorySourceResponse::ApiImport => Self::ApiImport,
            ExportMemorySourceResponse::System => Self::System,
        }
    }
}

impl TryFrom<ExportFactItemResponse> for Fact {
    type Error = memcore_common::MemcoreError;

    fn try_from(item: ExportFactItemResponse) -> MemcoreResult<Self> {
        Fact::new(
            item.id,
            item.org_id,
            item.user_id,
            item.memory_type.into(),
            item.content,
            item.summary,
            item.source.into(),
            item.confidence,
            item.importance,
            item.valid_at,
            item.invalid_at,
            item.recorded_at,
            item.updated_at,
            item.metadata,
        )
    }
}

/// Converts an API export payload (from `GET .../export`) into the core import model.
pub fn user_memory_export_from_response(
    response: UserMemoryExportResponse,
) -> MemcoreResult<UserMemoryExport> {
    let org_id = response.org_id.clone();
    let user_id = response.user_id.clone();

    let facts = response
        .facts
        .into_iter()
        .map(Fact::try_from)
        .collect::<MemcoreResult<Vec<_>>>()?;

    let memory_events = response
        .memory_events
        .into_iter()
        .map(|event| memory_event_from_api_item(event, &org_id, &user_id))
        .collect();

    Ok(UserMemoryExport {
        org_id: response.org_id,
        user_id: response.user_id,
        exported_at: response.exported_at,
        format_version: response.format_version,
        facts,
        memory_events,
    })
}

fn memory_event_from_api_item(
    item: MemoryEventItemResponse,
    org_id: &str,
    user_id: &str,
) -> MemoryEvent {
    let operation: memcore_core::MemoryEventOperation = item.operation.into();
    MemoryEvent {
        id: item.id,
        org_id: org_id.to_string(),
        user_id: user_id.to_string(),
        fact_id: item.fact_id,
        operation,
        input_text: None,
        previous_content: item.previous_content,
        new_content: item.new_content,
        provider_name: item.provider_name,
        model_name: item.model_name,
        metadata: item.metadata,
        created_at: item.created_at,
    }
}

impl From<MemoryEventOperationResponse> for memcore_core::MemoryEventOperation {
    fn from(value: MemoryEventOperationResponse) -> Self {
        match value {
            MemoryEventOperationResponse::Add => Self::Add,
            MemoryEventOperationResponse::Update => Self::Update,
            MemoryEventOperationResponse::Delete => Self::Delete,
            MemoryEventOperationResponse::NoOp => Self::NoOp,
            MemoryEventOperationResponse::ForgetUser => Self::ForgetUser,
        }
    }
}

impl From<UserMemoryExport> for ExportUserResponse {
    fn from(export: UserMemoryExport) -> Self {
        Self {
            status: "success",
            export: UserMemoryExportResponse {
                org_id: export.org_id,
                user_id: export.user_id,
                exported_at: export.exported_at,
                format_version: export.format_version,
                facts: export.facts.iter().map(ExportFactItemResponse::from).collect(),
                memory_events: export
                    .memory_events
                    .iter()
                    .map(MemoryEventItemResponse::from)
                    .collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memcore_core::{MemoryEventOperation, USER_EXPORT_FORMAT_VERSION};

    #[test]
    fn export_response_uses_format_version_constant() {
        let export = UserMemoryExport::new("org_a", "user_a", Vec::new(), Vec::new());
        let response = ExportUserResponse::from(export);
        assert_eq!(response.export.format_version, USER_EXPORT_FORMAT_VERSION);
    }

    #[test]
    fn memory_event_dto_omits_input_text() {
        let event = memcore_core::MemoryEvent::new(
            "org_a",
            "user_a",
            None,
            MemoryEventOperation::Add,
            None,
            Some("new".to_string()),
            None,
            None,
            serde_json::json!({}),
        );
        let dto = MemoryEventItemResponse::from(&event);
        let json = serde_json::to_value(&dto).expect("serialize");
        assert!(json.get("input_text").is_none());
    }
}
