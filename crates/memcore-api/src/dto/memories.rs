use memcore_common::MemcoreError;
use memcore_core::{
    AddMemoryOutput, DeleteMemoryOutput, Fact, ForgetUserOutput, ListMemoriesOutput,
    MemorySearchResult, MemoryType, SearchMemoryOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

pub fn default_search_limit() -> usize {
    memcore_core::DEFAULT_SEARCH_LIMIT
}

pub fn default_list_memories_limit() -> usize {
    memcore_core::DEFAULT_LIST_MEMORIES_LIMIT
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AddMemoryRequest {
    pub user_id: String,
    pub messages: Vec<MemoryMessageRequest>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct MemoryMessageRequest {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AddMemoryResponse {
    pub status: &'static str,
    pub summary: MemoryOperationSummaryResponse,
    pub memories: Vec<MemoryItemResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MemoryOperationSummaryResponse {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub noop: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MemoryItemResponse {
    pub id: Uuid,
    pub content: String,
    pub memory_type: MemoryTypeResponse,
    pub confidence: f32,
    pub importance: f32,
}

/// API-facing memory type labels (PascalCase) separate from core snake_case serde.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Deserialize, Default, ToSchema)]
pub struct SearchMemoryFiltersRequest {
    #[serde(default)]
    pub memory_type: Option<Vec<String>>,
}

impl SearchMemoryFiltersRequest {
    pub fn parse_memory_types(&self) -> Result<Option<Vec<MemoryType>>, MemcoreError> {
        let Some(types) = &self.memory_type else {
            return Ok(None);
        };

        if types.is_empty() {
            return Ok(None);
        }

        let parsed = types
            .iter()
            .map(|value| parse_memory_type_label(value))
            .collect::<Result<Vec<MemoryType>, _>>()?;
        Ok(Some(parsed))
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SearchMemoryRequest {
    pub user_id: String,
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    #[serde(default)]
    pub filters: SearchMemoryFiltersRequest,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchMemoryResponse {
    pub status: &'static str,
    pub results: Vec<SearchMemoryResultResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchMemoryResultResponse {
    pub fact_id: Uuid,
    pub content: String,
    pub memory_type: MemoryTypeResponse,
    pub score: f32,
    pub metadata: Value,
}

impl From<SearchMemoryOutput> for SearchMemoryResponse {
    fn from(output: SearchMemoryOutput) -> Self {
        Self {
            status: "success",
            results: output
                .results
                .iter()
                .map(SearchMemoryResultResponse::from)
                .collect(),
        }
    }
}

impl From<&MemorySearchResult> for SearchMemoryResultResponse {
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

#[derive(Debug, Clone, Deserialize)]
pub struct ListMemoriesQuery {
    pub memory_type: Option<String>,
    #[serde(default = "default_list_memories_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ListMemoriesResponse {
    pub status: &'static str,
    pub memories: Vec<ListMemoryItemResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ListMemoryItemResponse {
    pub id: Uuid,
    pub content: String,
    pub memory_type: MemoryTypeResponse,
    pub confidence: f32,
    pub importance: f32,
    pub metadata: Value,
}

impl From<ListMemoriesOutput> for ListMemoriesResponse {
    fn from(output: ListMemoriesOutput) -> Self {
        Self {
            status: "success",
            memories: output.memories.iter().map(ListMemoryItemResponse::from).collect(),
            next_cursor: output.next_cursor,
        }
    }
}

impl From<&Fact> for ListMemoryItemResponse {
    fn from(fact: &Fact) -> Self {
        Self {
            id: fact.id,
            content: fact.content.clone(),
            memory_type: fact.memory_type.into(),
            confidence: fact.confidence,
            importance: fact.importance,
            metadata: fact.metadata.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeleteMemoryResponse {
    pub status: &'static str,
    pub deleted: bool,
}

impl From<DeleteMemoryOutput> for DeleteMemoryResponse {
    fn from(output: DeleteMemoryOutput) -> Self {
        Self {
            status: "success",
            deleted: output.deleted,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ForgetUserResponse {
    pub status: &'static str,
    pub deleted: bool,
}

impl From<ForgetUserOutput> for ForgetUserResponse {
    fn from(output: ForgetUserOutput) -> Self {
        Self {
            status: "success",
            deleted: output.deleted,
        }
    }
}

pub fn parse_memory_type_label(label: &str) -> Result<MemoryType, MemcoreError> {
    match label.trim() {
        "Profile" => Ok(MemoryType::Profile),
        "Preference" => Ok(MemoryType::Preference),
        "Project" => Ok(MemoryType::Project),
        "Conversation" => Ok(MemoryType::Conversation),
        "Task" => Ok(MemoryType::Task),
        "Entity" => Ok(MemoryType::Entity),
        "Skill" => Ok(MemoryType::Skill),
        "System" => Ok(MemoryType::System),
        other => Err(MemcoreError::ValidationError(format!(
            "invalid memory type: {other}"
        ))),
    }
}
