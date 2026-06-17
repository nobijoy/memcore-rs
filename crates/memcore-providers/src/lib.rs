pub mod circuit_breaker;
pub mod factory;
pub mod inputs;
pub mod mocks;
pub mod openai;
pub mod policy;
pub mod resilient;
pub mod routing;
pub mod traits;

pub use circuit_breaker::{
    validate_circuit_breaker_config, CircuitBreakerConfig, CircuitBreakerSnapshot, CircuitState,
    ProviderCircuitBreaker,
};
pub use factory::{
    parse_provider_fallback_order, validate_embedding_provider_name,
    validate_llm_provider_name, validate_provider_fallback_order, validate_summarizer_provider_name,
};
pub use inputs::{
    FactClassificationInput, FactExtractionInput, MemoryMessage, MessageRole, SummarizationInput,
};
pub use mocks::{MockEmbeddingProvider, MockLlmProvider, deterministic_embedding};
pub use openai::{
    OpenAiClient, OpenAiEmbeddingProvider, OpenAiLlmProvider, default_embedding_dimensions_for_model,
};
pub use policy::{
    backoff_duration, execute_provider_call, is_provider_health_failure,
    is_retryable_provider_error, provider_timeout_error, validate_provider_execution_config,
    ProviderExecutionPolicy, ProviderRetryDecision,
};
pub use resilient::{
    build_resilient_embedding_from_candidates, build_resilient_llm_from_candidates,
    wrap_embedding_provider, wrap_llm_provider, PolicyEmbeddingProvider, PolicyLlmProvider,
    ResilientEmbeddingProvider, ResilientLlmProvider,
};
pub use routing::{
    circuit_key, ProviderCallContext, ProviderCandidate, ProviderCapability, ProviderFallbackRouter,
    ProviderId, ProviderRoutingMetrics, ProviderRoutingMetricsSnapshot,
};
pub use traits::{EmbeddingProvider, LlmProvider};
