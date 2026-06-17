use std::sync::Arc;

use memcore_common::MemcoreError;
use memcore_core::{
    AddMemoryInput, MemoryEngine, MemoryMessage, MessageRole, SearchMemoryInput, TenantContext,
};
use memcore_providers::{
    build_resilient_embedding_from_candidates, build_resilient_llm_from_candidates,
    CircuitBreakerConfig, MockEmbeddingProvider, MockLlmProvider, ProviderCandidate,
    ProviderCapability, ProviderCircuitBreaker, ProviderExecutionPolicy, ProviderId,
    ProviderRoutingMetrics,
};
use memcore_storage::{MockFactStore, MockVectorStore};
use serde_json::json;

fn tenant() -> TenantContext {
    TenantContext::new("org_fb", "user_fb").expect("tenant")
}

fn fast_policy() -> ProviderExecutionPolicy {
    ProviderExecutionPolicy {
        max_retries: 0,
        timeout: std::time::Duration::from_millis(200),
        initial_backoff: std::time::Duration::from_millis(1),
        max_backoff: std::time::Duration::from_millis(1),
        jitter_enabled: false,
        backoff_multiplier: 2.0,
    }
}

fn test_circuit_breaker() -> Arc<ProviderCircuitBreaker> {
    Arc::new(ProviderCircuitBreaker::new(CircuitBreakerConfig::for_tests()))
}

#[tokio::test]
async fn llm_extraction_falls_back_to_healthy_mock_provider() {
    let failing = Arc::new(MockLlmProvider::new().with_fail_error(MemcoreError::ProviderError(
        "OpenAI API error (503): unavailable".to_string(),
    )));
    let healthy = Arc::new(MockLlmProvider::new());
    let llm = build_resilient_llm_from_candidates(
        vec![
            ProviderCandidate {
                provider_id: ProviderId::new("primary", ProviderCapability::Llm),
                provider: failing,
            },
            ProviderCandidate {
                provider_id: ProviderId::new("secondary", ProviderCapability::Llm),
                provider: healthy,
            },
        ],
        vec![],
        test_circuit_breaker(),
        fast_policy(),
        true,
        Some(ProviderRoutingMetrics::new()),
    );

    let engine = MemoryEngine::new(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        llm,
        Arc::new(MockEmbeddingProvider::new(4)),
    );

    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "User enjoys hiking.".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("fallback add should succeed");

    assert_eq!(output.added, 1);
}

#[tokio::test]
async fn embedding_search_falls_back_to_healthy_mock_provider() {
    let failing = Arc::new(
        MockEmbeddingProvider::new(4).with_fail_error(MemcoreError::ProviderError(
            "OpenAI API error (500): internal".to_string(),
        )),
    );
    let healthy = Arc::new(MockEmbeddingProvider::new(4));
    let embedding = build_resilient_embedding_from_candidates(
        vec![
            ProviderCandidate {
                provider_id: ProviderId::new("primary", ProviderCapability::Embedding),
                provider: failing,
            },
            ProviderCandidate {
                provider_id: ProviderId::new("secondary", ProviderCapability::Embedding),
                provider: healthy,
            },
        ],
        test_circuit_breaker(),
        fast_policy(),
        true,
        Some(ProviderRoutingMetrics::new()),
    )
    .expect("embedding provider");

    let engine = MemoryEngine::new(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        Arc::new(MockLlmProvider::new()),
        embedding,
    );

    let _ = engine
        .search_memory(SearchMemoryInput {
            tenant: tenant(),
            query: "hiking".to_string(),
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search should succeed via fallback embedding");
}
