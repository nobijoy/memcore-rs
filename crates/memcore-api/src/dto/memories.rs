use memcore_core::{AddMemoryOutput, Fact, MemoryType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct AddMemoryRequest {
    pub user_id: String,
    pub messages: Vec<MemoryMessageRequest>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryMessageRequest {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddMemoryResponse {
    pub status: &'static str,
    pub summary: MemoryOperationSummaryResponse,
    pub memories: Vec<MemoryItemResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryOperationSummaryResponse {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub noop: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryItemResponse {
    pub id: Uuid,
    pub content: String,
    pub memory_type: MemoryTypeResponse,
    pub confidence: f32,
    pub importance: f32,
}

/// API-facing memory type labels (PascalCase) separate from core snake_case serde.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum MemoryTypeResponse {
    Profile,
    Preference,
    Project,
    Conversation,
    Task,
    Entity,
    Skill,
    System,
}

impl From<MemoryType> for MemoryTypeResponse {
    fn from(value: MemoryType) -> Self {
        match value {
            MemoryType::Profile => Self::Profile,
            MemoryType::Preference => Self::Preference,
            MemoryType::Project => Self::Project,
            MemoryType::Conversation => Self::Conversation,
            MemoryType::Task => Self::Task,
            MemoryType::Entity => Self::Entity,
            MemoryType::Skill => Self::Skill,
            MemoryType::System => Self::System,
        }
    }
}

impl From<&Fact> for MemoryItemResponse {
    fn from(fact: &Fact) -> Self {
        Self {
            id: fact.id,
            content: fact.content.clone(),
            memory_type: fact.memory_type.into(),
            confidence: fact.confidence,
            importance: fact.importance,
        }
    }
}

impl From<AddMemoryOutput> for AddMemoryResponse {
    fn from(output: AddMemoryOutput) -> Self {
        Self {
            status: "success",
            summary: MemoryOperationSummaryResponse {
                added: output.added,
                updated: output.updated,
                deleted: output.deleted,
                noop: output.noop,
            },
            memories: output.memories.iter().map(MemoryItemResponse::from).collect(),
        }
    }
}
