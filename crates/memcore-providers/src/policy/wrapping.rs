use std::sync::Arc;

use async_trait::async_trait;
use memcore_common::MemcoreResult;
use memcore_core::{CandidateFact, FactOperationDecision};

use crate::inputs::{
    FactClassificationInput, FactExtractionInput, SummarizationInput,
};
use crate::traits::{EmbeddingProvider, LlmProvider};

use super::execute::execute_provider_call;
use super::ProviderExecutionPolicy;

/// LLM provider wrapper applying timeout and bounded retry policy to all calls.
pub struct PolicyLlmProvider {
    inner: Arc<dyn LlmProvider>,
    policy: ProviderExecutionPolicy,
}

impl PolicyLlmProvider {
    pub fn new(inner: Arc<dyn LlmProvider>, policy: ProviderExecutionPolicy) -> Self {
        Self { inner, policy }
    }
}

pub fn wrap_llm_provider(
    inner: Arc<dyn LlmProvider>,
    policy: ProviderExecutionPolicy,
) -> Arc<dyn LlmProvider> {
    Arc::new(PolicyLlmProvider::new(inner, policy))
}

#[async_trait]
impl LlmProvider for PolicyLlmProvider {
    async fn extract_facts(
        &self,
        input: FactExtractionInput,
    ) -> MemcoreResult<Vec<CandidateFact>> {
        let inner = self.inner.clone();
        let policy = self.policy.clone();
        execute_provider_call("llm_extract_facts", &policy, || {
            let inner = inner.clone();
            let input = input.clone();
            async move { inner.extract_facts(input).await }
        })
        .await
    }

    async fn classify_fact_operation(
        &self,
        input: FactClassificationInput,
    ) -> MemcoreResult<FactOperationDecision> {
        let inner = self.inner.clone();
        let policy = self.policy.clone();
        execute_provider_call("llm_classify_fact_operation", &policy, || {
            let inner = inner.clone();
            let input = input.clone();
            async move { inner.classify_fact_operation(input).await }
        })
        .await
    }

    async fn summarize_memory(&self, input: SummarizationInput) -> MemcoreResult<String> {
        let inner = self.inner.clone();
        let policy = self.policy.clone();
        execute_provider_call("llm_summarize_memory", &policy, || {
            let inner = inner.clone();
            let input = input.clone();
            async move { inner.summarize_memory(input).await }
        })
        .await
    }
}

/// Embedding provider wrapper applying timeout and bounded retry policy to all calls.
pub struct PolicyEmbeddingProvider {
    inner: Arc<dyn EmbeddingProvider>,
    policy: ProviderExecutionPolicy,
}

impl PolicyEmbeddingProvider {
    pub fn new(inner: Arc<dyn EmbeddingProvider>, policy: ProviderExecutionPolicy) -> Self {
        Self { inner, policy }
    }
}

pub fn wrap_embedding_provider(
    inner: Arc<dyn EmbeddingProvider>,
    policy: ProviderExecutionPolicy,
) -> Arc<dyn EmbeddingProvider> {
    Arc::new(PolicyEmbeddingProvider::new(inner, policy))
}

#[async_trait]
impl EmbeddingProvider for PolicyEmbeddingProvider {
    async fn embed_text(&self, text: &str) -> MemcoreResult<Vec<f32>> {
        let inner = self.inner.clone();
        let policy = self.policy.clone();
        let text = text.to_string();
        execute_provider_call("embedding_embed_text", &policy, || {
            let inner = inner.clone();
            let text = text.clone();
            async move { inner.embed_text(&text).await }
        })
        .await
    }

    async fn embed_batch(&self, texts: Vec<String>) -> MemcoreResult<Vec<Vec<f32>>> {
        let inner = self.inner.clone();
        let policy = self.policy.clone();
        execute_provider_call("embedding_embed_batch", &policy, || {
            let inner = inner.clone();
            let texts = texts.clone();
            async move { inner.embed_batch(texts).await }
        })
        .await
    }

    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }
}
