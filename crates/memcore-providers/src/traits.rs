use async_trait::async_trait;
use memcore_common::MemcoreResult;
use memcore_core::{CandidateFact, FactOperationDecision};

use crate::inputs::{FactClassificationInput, FactExtractionInput, SummarizationInput};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn extract_facts(
        &self,
        input: FactExtractionInput,
    ) -> MemcoreResult<Vec<CandidateFact>>;

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
