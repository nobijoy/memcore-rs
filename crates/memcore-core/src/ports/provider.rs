use async_trait::async_trait;
use memcore_common::MemcoreResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CandidateFact, Fact, FactOperationDecision, TenantContext};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactExtractionInput {
    pub tenant: TenantContext,
    pub messages: Vec<MemoryMessage>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactClassificationInput {
    pub tenant: TenantContext,
    pub candidate_fact: CandidateFact,
    pub existing_facts: Vec<Fact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummarizationInput {
    pub tenant: TenantContext,
    pub facts: Vec<Fact>,
    pub max_tokens: Option<usize>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn extract_facts(&self, input: FactExtractionInput) -> MemcoreResult<Vec<CandidateFact>>;

    async fn classify_fact_operation(
        &self,
        input: FactClassificationInput,
    ) -> MemcoreResult<FactOperationDecision>;

    async fn summarize_memory(&self, input: SummarizationInput) -> MemcoreResult<String>;
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed_text(&self, text: &str) -> MemcoreResult<Vec<f32>>;

    async fn embed_batch(&self, texts: Vec<String>) -> MemcoreResult<Vec<Vec<f32>>>;

    fn dimensions(&self) -> usize;
}
