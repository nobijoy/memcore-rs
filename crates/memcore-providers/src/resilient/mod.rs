mod compat;

use std::sync::Arc;

use async_trait::async_trait;
use memcore_common::MemcoreResult;
use memcore_core::{CandidateFact, FactOperationDecision};

use crate::circuit_breaker::ProviderCircuitBreaker;
use crate::inputs::{
    FactClassificationInput, FactExtractionInput, SummarizationInput,
};
use crate::policy::ProviderExecutionPolicy;
use crate::routing::{
    ProviderCandidate, ProviderCapability, ProviderFallbackRouter,
    ProviderRoutingMetrics,
};
use crate::traits::{EmbeddingProvider, LlmProvider};
use crate::usage::{
    estimate_embedding_batch_tokens, estimate_embedding_tokens, estimate_llm_classification_tokens,
    estimate_llm_extraction_tokens, estimate_llm_summarization_tokens, store_token_usage,
    ProviderUsageRecorder, TokenUsageSlot,
};

/// LLM provider with timeout/retry, circuit breaker, optional fallback routing, and usage recording.
pub struct ResilientLlmProvider {
    providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
    summarizer_providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
    router: ProviderFallbackRouter,
    fallback_enabled: bool,
}

impl ResilientLlmProvider {
    pub fn new(
        providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
        summarizer_providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
        circuit_breaker: Arc<ProviderCircuitBreaker>,
        policy: ProviderExecutionPolicy,
        fallback_enabled: bool,
        metrics: Option<Arc<ProviderRoutingMetrics>>,
        usage_recorder: Option<Arc<dyn ProviderUsageRecorder>>,
        cost_tracking_enabled: bool,
    ) -> Self {
        Self {
            providers,
            summarizer_providers,
            router: ProviderFallbackRouter::new(
                circuit_breaker,
                policy,
                metrics,
                usage_recorder,
                cost_tracking_enabled,
            ),
            fallback_enabled,
        }
    }
}

pub fn build_resilient_llm_provider(
    providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
    summarizer_providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    policy: ProviderExecutionPolicy,
    fallback_enabled: bool,
    metrics: Option<Arc<ProviderRoutingMetrics>>,
    usage_recorder: Option<Arc<dyn ProviderUsageRecorder>>,
    cost_tracking_enabled: bool,
) -> Arc<dyn LlmProvider> {
    Arc::new(ResilientLlmProvider::new(
        providers,
        summarizer_providers,
        circuit_breaker,
        policy,
        fallback_enabled,
        metrics,
        usage_recorder,
        cost_tracking_enabled,
    ))
}

fn store_estimated_usage(slot: Option<TokenUsageSlot>, usage: crate::usage::ProviderTokenUsage) {
    if let Some(slot) = slot {
        store_token_usage(&slot, usage);
    }
}

#[async_trait]
impl LlmProvider for ResilientLlmProvider {
    async fn extract_facts(
        &self,
        input: FactExtractionInput,
    ) -> MemcoreResult<Vec<CandidateFact>> {
        self.router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "llm_extract_facts",
                self.fallback_enabled,
                &self.providers,
                |provider, slot| {
                    let input = input.clone();
                    async move {
                        let facts = provider.extract_facts(input.clone()).await?;
                        store_estimated_usage(slot, estimate_llm_extraction_tokens(&input));
                        Ok(facts)
                    }
                },
            )
            .await
    }

    async fn classify_fact_operation(
        &self,
        input: FactClassificationInput,
    ) -> MemcoreResult<FactOperationDecision> {
        self.router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "llm_classify_fact_operation",
                self.fallback_enabled,
                &self.providers,
                |provider, slot| {
                    let input = input.clone();
                    async move {
                        let decision = provider.classify_fact_operation(input.clone()).await?;
                        store_estimated_usage(
                            slot,
                            estimate_llm_classification_tokens(
                                &input.candidate_fact.content,
                                input.existing_facts.len(),
                            ),
                        );
                        Ok(decision)
                    }
                },
            )
            .await
    }

    async fn summarize_memory(&self, input: SummarizationInput) -> MemcoreResult<String> {
        let providers = if self.summarizer_providers.is_empty() {
            &self.providers
        } else {
            &self.summarizer_providers
        };
        self.router
            .execute_with_fallback(
                ProviderCapability::Summarization,
                "llm_summarize_memory",
                self.fallback_enabled,
                providers,
                |provider, slot| {
                    let input = input.clone();
                    async move {
                        let summary = provider.summarize_memory(input.clone()).await?;
                        store_estimated_usage(slot, estimate_llm_summarization_tokens(&input));
                        Ok(summary)
                    }
                },
            )
            .await
    }
}

/// Embedding provider with timeout/retry, circuit breaker, optional fallback routing, and usage recording.
pub struct ResilientEmbeddingProvider {
    providers: Vec<ProviderCandidate<Arc<dyn EmbeddingProvider>>>,
    router: ProviderFallbackRouter,
    fallback_enabled: bool,
    dimensions: usize,
}

impl ResilientEmbeddingProvider {
    pub fn new(
        providers: Vec<ProviderCandidate<Arc<dyn EmbeddingProvider>>>,
        circuit_breaker: Arc<ProviderCircuitBreaker>,
        policy: ProviderExecutionPolicy,
        fallback_enabled: bool,
        metrics: Option<Arc<ProviderRoutingMetrics>>,
        usage_recorder: Option<Arc<dyn ProviderUsageRecorder>>,
        cost_tracking_enabled: bool,
    ) -> MemcoreResult<Self> {
        let dimensions = providers
            .first()
            .map(|candidate| candidate.provider.dimensions())
            .ok_or_else(|| {
                memcore_common::MemcoreError::ValidationError(
                    "at least one embedding provider is required".to_string(),
                )
            })?;

        for candidate in providers.iter().skip(1) {
            if candidate.provider.dimensions() != dimensions {
                return Err(memcore_common::MemcoreError::ValidationError(
                    "all embedding providers in fallback order must share the same dimensions"
                        .to_string(),
                ));
            }
        }

        Ok(Self {
            providers,
            router: ProviderFallbackRouter::new(
                circuit_breaker,
                policy,
                metrics,
                usage_recorder,
                cost_tracking_enabled,
            ),
            fallback_enabled,
            dimensions,
        })
    }
}

pub fn build_resilient_embedding_provider(
    providers: Vec<ProviderCandidate<Arc<dyn EmbeddingProvider>>>,
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    policy: ProviderExecutionPolicy,
    fallback_enabled: bool,
    metrics: Option<Arc<ProviderRoutingMetrics>>,
    usage_recorder: Option<Arc<dyn ProviderUsageRecorder>>,
    cost_tracking_enabled: bool,
) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    Ok(Arc::new(ResilientEmbeddingProvider::new(
        providers,
        circuit_breaker,
        policy,
        fallback_enabled,
        metrics,
        usage_recorder,
        cost_tracking_enabled,
    )?))
}

#[async_trait]
impl EmbeddingProvider for ResilientEmbeddingProvider {
    async fn embed_text(&self, text: &str) -> MemcoreResult<Vec<f32>> {
        let text = text.to_string();
        self.router
            .execute_with_fallback(
                ProviderCapability::Embedding,
                "embedding_embed_text",
                self.fallback_enabled,
                &self.providers,
                |provider, slot| {
                    let text = text.clone();
                    async move {
                        let embedding = provider.embed_text(&text).await?;
                        store_estimated_usage(slot, estimate_embedding_tokens(&text));
                        Ok(embedding)
                    }
                },
            )
            .await
    }

    async fn embed_batch(&self, texts: Vec<String>) -> MemcoreResult<Vec<Vec<f32>>> {
        self.router
            .execute_with_fallback(
                ProviderCapability::Embedding,
                "embedding_embed_batch",
                self.fallback_enabled,
                &self.providers,
                |provider, slot| {
                    let texts = texts.clone();
                    async move {
                        let embeddings = provider.embed_batch(texts.clone()).await?;
                        store_estimated_usage(slot, estimate_embedding_batch_tokens(&texts));
                        Ok(embeddings)
                    }
                },
            )
            .await
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

pub use compat::{
    build_resilient_embedding_from_candidates, build_resilient_llm_from_candidates,
    wrap_embedding_provider, wrap_llm_provider, PolicyEmbeddingProvider, PolicyLlmProvider,
};
