use chrono::{DateTime, Utc};
use memcore_core::{Fact, MemorySource, UserMemoryExport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::memories::MemoryTypeResponse;
use super::memory_events::MemoryEventItemResponse;

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

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserMemoryExportResponse {
    pub org_id: String,
    pub user_id: String,
    pub exported_at: DateTime<Utc>,
    pub format_version: String,
    pub facts: Vec<ExportFactItemResponse>,
    pub memory_events: Vec<MemoryEventItemResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
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

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
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
