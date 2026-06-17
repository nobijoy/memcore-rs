use std::sync::Arc;

use memcore_common::MemcoreResult;

use crate::circuit_breaker::{CircuitBreakerConfig, ProviderCircuitBreaker};
use crate::policy::ProviderExecutionPolicy;
use crate::routing::{ProviderCandidate, ProviderCapability, ProviderId, ProviderRoutingMetrics};
use crate::traits::{EmbeddingProvider, LlmProvider};

use super::{build_resilient_embedding_provider, build_resilient_llm_provider};

pub use super::{ResilientEmbeddingProvider as PolicyEmbeddingProvider, ResilientLlmProvider as PolicyLlmProvider};

fn disabled_circuit_breaker() -> Arc<ProviderCircuitBreaker> {
    Arc::new(ProviderCircuitBreaker::new(CircuitBreakerConfig {
        enabled: false,
        ..CircuitBreakerConfig::default()
    }))
}

/// Backward-compatible single-provider wrapper (circuit breaker and fallback disabled).
pub fn wrap_llm_provider(
    inner: Arc<dyn LlmProvider>,
    policy: ProviderExecutionPolicy,
) -> Arc<dyn LlmProvider> {
    let providers = vec![ProviderCandidate {
        provider_id: ProviderId::new("primary", ProviderCapability::Llm),
        provider: inner.clone(),
    }];
    let summarizer_providers = vec![ProviderCandidate {
        provider_id: ProviderId::new("primary", ProviderCapability::Summarization),
        provider: inner,
    }];
    build_resilient_llm_provider(
        providers,
        summarizer_providers,
        disabled_circuit_breaker(),
        policy,
        false,
        None,
    )
}

/// Backward-compatible single-provider wrapper (circuit breaker and fallback disabled).
pub fn wrap_embedding_provider(
    inner: Arc<dyn EmbeddingProvider>,
    policy: ProviderExecutionPolicy,
) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    let providers = vec![ProviderCandidate {
        provider_id: ProviderId::new("primary", ProviderCapability::Embedding),
        provider: inner,
    }];
    build_resilient_embedding_provider(
        providers,
        disabled_circuit_breaker(),
        policy,
        false,
        None,
    )
}

pub fn build_resilient_llm_from_candidates(
    providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
    summarizer_providers: Vec<ProviderCandidate<Arc<dyn LlmProvider>>>,
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    policy: ProviderExecutionPolicy,
    fallback_enabled: bool,
    metrics: Option<Arc<ProviderRoutingMetrics>>,
) -> Arc<dyn LlmProvider> {
    build_resilient_llm_provider(
        providers,
        summarizer_providers,
        circuit_breaker,
        policy,
        fallback_enabled,
        metrics,
    )
}

pub fn build_resilient_embedding_from_candidates(
    providers: Vec<ProviderCandidate<Arc<dyn EmbeddingProvider>>>,
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    policy: ProviderExecutionPolicy,
    fallback_enabled: bool,
    metrics: Option<Arc<ProviderRoutingMetrics>>,
) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    build_resilient_embedding_provider(
        providers,
        circuit_breaker,
        policy,
        fallback_enabled,
        metrics,
    )
}
