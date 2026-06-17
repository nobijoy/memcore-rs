use std::sync::Arc;

use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_config::{
    AuthMode, ContextCacheBackend, EmbeddingProviderKind, EventBackend, FactBackend,
    LlmProviderKind, Settings, VectorBackend,
};
use memcore_core::{
    ApiKeyStore, ContextCacheConfig, EmbeddingProvider, FactStore, InMemoryContextCache,
    LlmProvider, MemoryEngine, MemoryEventStore, VectorStore,
};
use memcore_providers::{
    build_resilient_embedding_from_candidates, build_resilient_llm_from_candidates,
    validate_embedding_provider_name, validate_llm_provider_name, validate_provider_fallback_order,
    validate_summarizer_provider_name, CircuitBreakerConfig, MockEmbeddingProvider,
    MockLlmProvider, OpenAiClient, OpenAiEmbeddingProvider, OpenAiLlmProvider, ProviderCandidate,
    ProviderCapability, ProviderCircuitBreaker, ProviderExecutionPolicy, ProviderId,
    ProviderRoutingMetrics, default_embedding_dimensions_for_model,
};
use memcore_storage::{
    MockApiKeyStore, MockFactStore, MockMemoryEventStore, MockVectorStore, SqliteApiKeyStore,
    SqliteFactStore, SqliteMemoryEventStore,
};
#[cfg(feature = "postgres")]
use memcore_storage::{PostgresApiKeyStore, PostgresFactStore, PostgresMemoryEventStore};
use crate::middleware::RateLimiter;
use crate::observability::Metrics;
#[cfg(feature = "lancedb")]
use memcore_storage::LanceDbVectorStore;
#[cfg(feature = "qdrant")]
use memcore_storage::QdrantVectorStore;
#[cfg(feature = "redis-cache")]
use memcore_storage::RedisContextCache;

/// Default embedding dimensions for the mock embedding provider.
const MOCK_EMBEDDING_DIMENSIONS: usize = 8;

struct ProviderRuntime {
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    metrics: Arc<ProviderRoutingMetrics>,
    policy: ProviderExecutionPolicy,
}

fn provider_runtime(settings: &Settings) -> MemcoreResult<ProviderRuntime> {
    Ok(ProviderRuntime {
        circuit_breaker: Arc::new(ProviderCircuitBreaker::new(
            CircuitBreakerConfig::from_config(
                settings.provider_circuit_breaker_enabled,
                settings.provider_circuit_breaker_failure_threshold,
                settings.provider_circuit_breaker_reset_timeout_seconds,
                settings.provider_circuit_breaker_half_open_max_calls,
            )?,
        )),
        metrics: ProviderRoutingMetrics::new(),
        policy: provider_execution_policy(settings)?,
    })
}

fn llm_provider_kind_name(kind: &LlmProviderKind) -> &'static str {
    match kind {
        LlmProviderKind::Mock => "mock",
        LlmProviderKind::OpenAi => "openai",
        LlmProviderKind::OpenRouter => "openrouter",
        LlmProviderKind::Anthropic => "anthropic",
        LlmProviderKind::Groq => "groq",
    }
}

fn embedding_provider_kind_name(kind: &EmbeddingProviderKind) -> &'static str {
    match kind {
        EmbeddingProviderKind::Mock => "mock",
        EmbeddingProviderKind::OpenAi => "openai",
    }
}

fn build_llm_provider_by_name(
    name: &str,
    settings: &Settings,
) -> MemcoreResult<Arc<dyn LlmProvider>> {
    match name {
        "mock" => Ok(Arc::new(MockLlmProvider::new())),
        "openai" => {
            let api_key = require_openai_api_key(
                settings,
                "OPENAI_API_KEY is required when MEMCORE_LLM_PROVIDER=openai",
            )?;
            let client = OpenAiClient::new(&api_key, &settings.openai_base_url)
                .map_err(|err| provider_init_error("OpenAI LLM", err))?;
            Ok(Arc::new(OpenAiLlmProvider::new(client, settings.llm_model.clone())))
        }
        "openrouter" => Err(MemcoreError::ValidationError(
            "openrouter LLM provider is not wired into the API yet".to_string(),
        )),
        "anthropic" => Err(MemcoreError::ValidationError(
            "anthropic LLM provider is not wired into the API yet".to_string(),
        )),
        "groq" => Err(MemcoreError::ValidationError(
            "groq LLM provider is not wired into the API yet".to_string(),
        )),
        _ => Err(MemcoreError::ValidationError(format!(
            "unknown LLM provider in fallback order: {name}"
        ))),
    }
}

fn build_embedding_provider_by_name(
    name: &str,
    settings: &Settings,
) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    match name {
        "mock" => Ok(Arc::new(MockEmbeddingProvider::new(MOCK_EMBEDDING_DIMENSIONS))),
        "openai" => {
            let api_key = require_openai_api_key(
                settings,
                "OPENAI_API_KEY is required when MEMCORE_EMBEDDING_PROVIDER=openai",
            )?;
            let client = OpenAiClient::new(&api_key, &settings.openai_base_url)
                .map_err(|err| provider_init_error("OpenAI embedding", err))?;
            let dimensions =
                default_embedding_dimensions_for_model(&settings.embedding_model);
            let provider = OpenAiEmbeddingProvider::new(
                client,
                settings.embedding_model.clone(),
                dimensions,
            )
            .map_err(|err| provider_init_error("OpenAI embedding", err))?;
            Ok(Arc::new(provider))
        }
        _ => Err(MemcoreError::ValidationError(format!(
            "unknown embedding provider in fallback order: {name}"
        ))),
    }
}

fn llm_candidates_from_names(
    names: &[String],
    capability: ProviderCapability,
    settings: &Settings,
) -> MemcoreResult<Vec<ProviderCandidate<Arc<dyn LlmProvider>>>> {
    names
        .iter()
        .map(|name| {
            Ok(ProviderCandidate {
                provider_id: ProviderId::new(name.clone(), capability),
                provider: build_llm_provider_by_name(name, settings)?,
            })
        })
        .collect()
}

fn embedding_candidates_from_names(
    names: &[String],
    settings: &Settings,
) -> MemcoreResult<Vec<ProviderCandidate<Arc<dyn EmbeddingProvider>>>> {
    names
        .iter()
        .map(|name| {
            Ok(ProviderCandidate {
                provider_id: ProviderId::new(name.clone(), ProviderCapability::Embedding),
                provider: build_embedding_provider_by_name(name, settings)?,
            })
        })
        .collect()
}

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub started_at: DateTime<Utc>,
    pub memory_engine: Arc<MemoryEngine>,
    pub api_key_store: Arc<dyn ApiKeyStore>,
    pub rate_limiter: Arc<RateLimiter>,
    pub metrics: Arc<Metrics>,
}

impl AppState {
    /// Builds application state using configured storage and providers.
    pub async fn initialize(settings: Settings) -> MemcoreResult<Self> {
        let memory_engine = Arc::new(create_memory_engine(&settings).await?);
        let api_key_store = create_api_key_store(&settings).await?;
        Ok(Self {
            settings: settings.clone(),
            started_at: Utc::now(),
            memory_engine,
            api_key_store,
            rate_limiter: create_rate_limiter(&settings),
            metrics: Arc::new(Metrics::default()),
        })
    }

    /// Synchronous helper for tests when fact, event, and vector backends are all mock.
    ///
    /// For SQLite, Postgres, or LanceDB backends, call [`Self::initialize`] from async code instead.
    pub fn new(settings: Settings) -> Self {
        if settings.fact_backend == FactBackend::Mock
            && settings.event_backend == EventBackend::Mock
            && settings.vector_backend == VectorBackend::Mock
        {
            Self {
                started_at: Utc::now(),
                memory_engine: Arc::new(
                    create_mock_memory_engine(&settings)
                        .expect("failed to create mock memory engine"),
                ),
                api_key_store: Arc::new(MockApiKeyStore::new()),
                settings: settings.clone(),
                rate_limiter: create_rate_limiter(&settings),
                metrics: Arc::new(Metrics::default()),
            }
        } else {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(Self::initialize(settings))
            })
            .expect("failed to initialize AppState")
        }
    }

    pub fn with_memory_engine(settings: Settings, memory_engine: Arc<MemoryEngine>) -> Self {
        Self {
            settings: settings.clone(),
            started_at: Utc::now(),
            memory_engine,
            api_key_store: Arc::new(MockApiKeyStore::new()),
            rate_limiter: create_rate_limiter(&settings),
            metrics: Arc::new(Metrics::default()),
        }
    }
}

fn create_rate_limiter(settings: &Settings) -> Arc<RateLimiter> {
    Arc::new(RateLimiter::new(
        settings.rate_limit_enabled,
        settings.rate_limit_requests_per_minute,
    ))
}

/// Wires `MemoryEngine` from settings: configurable fact/vector stores and LLM/embedding providers.
pub async fn create_memory_engine(settings: &Settings) -> MemcoreResult<MemoryEngine> {
    let (fact_store, event_store) = create_storage(settings).await?;
    let llm_provider = create_llm_provider(settings)?;
    let embedding_provider = create_embedding_provider(settings)?;
    let vector_store =
        create_vector_store(settings, embedding_provider.dimensions()).await?;

    Ok(apply_context_cache_async(
        MemoryEngine::new(
            fact_store,
            vector_store,
            llm_provider,
            embedding_provider,
        )
        .with_pii_redaction(settings.enable_pii_redaction)
        .with_event_store(event_store)
        .with_audit_provider_info(
            Some(llm_provider_name(settings)),
            Some(settings.llm_model.clone()),
        ),
        settings,
    )
    .await?)
}

async fn create_api_key_store(settings: &Settings) -> MemcoreResult<Arc<dyn ApiKeyStore>> {
    if settings.auth_mode == AuthMode::Dev {
        return Ok(Arc::new(MockApiKeyStore::new()));
    }

    #[cfg(feature = "postgres")]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        let postgres_url = require_postgres_url(settings)?;
        let store = PostgresApiKeyStore::connect(&postgres_url).await?;
        return Ok(Arc::new(store));
    }

    #[cfg(not(feature = "postgres"))]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        return Err(MemcoreError::ValidationError(
            "database auth with postgres storage requires the `postgres` cargo feature".to_string(),
        ));
    }

    let store = SqliteApiKeyStore::connect(&settings.database_url).await?;
    Ok(Arc::new(store))
}

async fn create_storage(
    settings: &Settings,
) -> MemcoreResult<(Arc<dyn FactStore>, Arc<dyn MemoryEventStore>)> {
    let needs_postgres = settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres;

    #[cfg(not(feature = "postgres"))]
    if needs_postgres {
        if settings.event_backend == EventBackend::Postgres {
            return Err(MemcoreError::ValidationError(
                "Postgres event backend requires the `postgres` cargo feature".to_string(),
            ));
        }
        return Err(MemcoreError::ValidationError(
            "Postgres fact backend requires the `postgres` cargo feature".to_string(),
        ));
    }

    match (&settings.fact_backend, &settings.event_backend) {
        (FactBackend::Mock, EventBackend::Mock) => Ok((
            Arc::new(MockFactStore::new()),
            Arc::new(MockMemoryEventStore::new()),
        )),
        (FactBackend::Sqlite, EventBackend::Sqlite) => {
            let fact_store = SqliteFactStore::connect(&settings.database_url).await?;
            let event_store = SqliteMemoryEventStore::new(fact_store.pool());
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        (FactBackend::Sqlite, EventBackend::Mock) => {
            let fact_store = SqliteFactStore::connect(&settings.database_url).await?;
            Ok((
                Arc::new(fact_store),
                Arc::new(MockMemoryEventStore::new()),
            ))
        }
        (FactBackend::Mock, EventBackend::Sqlite) => {
            let event_store =
                SqliteMemoryEventStore::connect(&settings.database_url).await?;
            Ok((
                Arc::new(MockFactStore::new()),
                Arc::new(event_store),
            ))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Postgres, EventBackend::Postgres) => {
            let postgres_url = require_postgres_url(settings)?;
            let fact_store = PostgresFactStore::connect(&postgres_url).await?;
            let event_store = PostgresMemoryEventStore::new(fact_store.pool());
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Postgres, EventBackend::Mock) => {
            let postgres_url = require_postgres_url(settings)?;
            let fact_store = PostgresFactStore::connect(&postgres_url).await?;
            Ok((
                Arc::new(fact_store),
                Arc::new(MockMemoryEventStore::new()),
            ))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Mock, EventBackend::Postgres) => {
            let postgres_url = require_postgres_url(settings)?;
            let event_store = PostgresMemoryEventStore::connect(&postgres_url).await?;
            Ok((
                Arc::new(MockFactStore::new()),
                Arc::new(event_store),
            ))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Sqlite, EventBackend::Postgres) => {
            let fact_store = SqliteFactStore::connect(&settings.database_url).await?;
            let postgres_url = require_postgres_url(settings)?;
            let event_store = PostgresMemoryEventStore::connect(&postgres_url).await?;
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Postgres, EventBackend::Sqlite) => {
            let postgres_url = require_postgres_url(settings)?;
            let fact_store = PostgresFactStore::connect(&postgres_url).await?;
            let event_store =
                SqliteMemoryEventStore::connect(&settings.database_url).await?;
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        #[cfg(not(feature = "postgres"))]
        (FactBackend::Postgres, _) | (_, EventBackend::Postgres) => Err(
            MemcoreError::ValidationError(
                "Postgres storage requires the `postgres` cargo feature".to_string(),
            ),
        ),
    }
}

#[cfg(feature = "postgres")]
fn require_postgres_url(settings: &Settings) -> MemcoreResult<String> {
    settings
        .postgres_url
        .as_ref()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            if settings.event_backend == EventBackend::Postgres
                && settings.fact_backend != FactBackend::Postgres
            {
                MemcoreError::ValidationError(
                    "MEMCORE_POSTGRES_URL is required when MEMCORE_EVENT_BACKEND=postgres"
                        .to_string(),
                )
            } else {
                MemcoreError::ValidationError(
                    "MEMCORE_POSTGRES_URL is required when MEMCORE_FACT_BACKEND=postgres"
                        .to_string(),
                )
            }
        })
}

fn provider_execution_policy(settings: &Settings) -> MemcoreResult<ProviderExecutionPolicy> {
    ProviderExecutionPolicy::from_config(
        settings.provider_timeout_seconds,
        settings.provider_max_retries,
        settings.provider_initial_backoff_ms,
        settings.provider_max_backoff_ms,
        settings.provider_backoff_multiplier,
        settings.provider_retry_jitter_enabled,
    )
}

fn create_llm_provider(settings: &Settings) -> MemcoreResult<Arc<dyn LlmProvider>> {
    let runtime = provider_runtime(settings)?;
    let llm_names = if settings.provider_fallback_enabled {
        settings.llm_fallback_order.clone()
    } else {
        vec![llm_provider_kind_name(&settings.llm_provider).to_string()]
    };
    let summarizer_names = if settings.provider_fallback_enabled {
        settings.summarizer_fallback_order.clone()
    } else {
        vec![llm_provider_kind_name(&settings.llm_provider).to_string()]
    };

    if settings.provider_fallback_enabled {
        validate_provider_fallback_order(&llm_names, validate_llm_provider_name)?;
        validate_provider_fallback_order(&summarizer_names, validate_summarizer_provider_name)?;
    } else if matches!(
        settings.llm_provider,
        LlmProviderKind::OpenRouter | LlmProviderKind::Anthropic | LlmProviderKind::Groq
    ) {
        return build_llm_provider_by_name(
            llm_provider_kind_name(&settings.llm_provider),
            settings,
        );
    }

    let providers = llm_candidates_from_names(&llm_names, ProviderCapability::Llm, settings)?;
    let summarizer_providers = llm_candidates_from_names(
        &summarizer_names,
        ProviderCapability::Summarization,
        settings,
    )?;

    Ok(build_resilient_llm_from_candidates(
        providers,
        summarizer_providers,
        runtime.circuit_breaker,
        runtime.policy,
        settings.provider_fallback_enabled,
        Some(runtime.metrics),
    ))
}

fn create_embedding_provider(settings: &Settings) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    let runtime = provider_runtime(settings)?;
    let names = if settings.provider_fallback_enabled {
        settings.embedding_fallback_order.clone()
    } else {
        vec![
            embedding_provider_kind_name(&settings.embedding_provider).to_string(),
        ]
    };

    if settings.provider_fallback_enabled {
        validate_provider_fallback_order(&names, validate_embedding_provider_name)?;
    }

    let providers = embedding_candidates_from_names(&names, settings)?;

    build_resilient_embedding_from_candidates(
        providers,
        runtime.circuit_breaker,
        runtime.policy,
        settings.provider_fallback_enabled,
        Some(runtime.metrics),
    )
}

fn require_openai_api_key(settings: &Settings, message: &str) -> MemcoreResult<String> {
    settings
        .openai_api_key
        .as_ref()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .ok_or_else(|| MemcoreError::ValidationError(message.to_string()))
}

fn provider_init_error(provider: &str, err: MemcoreError) -> MemcoreError {
    MemcoreError::ValidationError(format!("failed to initialize {provider} provider: {err}"))
}

async fn create_vector_store(
    settings: &Settings,
    dimensions: usize,
) -> MemcoreResult<Arc<dyn VectorStore>> {
    match settings.vector_backend {
        VectorBackend::Mock => Ok(Arc::new(MockVectorStore::new())),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                let store = LanceDbVectorStore::new_or_open(
                    &settings.lancedb_path,
                    &settings.lancedb_table,
                    dimensions,
                )
                .await?;
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "lancedb"))]
            {
                let _ = dimensions;
                Err(MemcoreError::ValidationError(
                    "LanceDB vector backend requires the `lancedb` cargo feature".to_string(),
                ))
            }
        }
        VectorBackend::Qdrant => {
            #[cfg(feature = "qdrant")]
            {
                let url = require_qdrant_url(settings)?;
                let store = QdrantVectorStore::connect(
                    &url,
                    &settings.qdrant_collection,
                    dimensions,
                )
                .await?;
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "qdrant"))]
            {
                let _ = dimensions;
                Err(MemcoreError::ValidationError(
                    "Qdrant vector backend requires the `qdrant` cargo feature".to_string(),
                ))
            }
        }
    }
}

#[cfg(feature = "qdrant")]
fn require_qdrant_url(settings: &Settings) -> MemcoreResult<String> {
    let url = settings.qdrant_url.trim();
    if url.is_empty() {
        return Err(MemcoreError::ValidationError(
            "MEMCORE_QDRANT_URL is required when MEMCORE_VECTOR_BACKEND=qdrant".to_string(),
        ));
    }
    Ok(url.to_string())
}

/// In-memory mock fact and vector stores for fast API tests.
pub fn create_mock_memory_engine(settings: &Settings) -> MemcoreResult<MemoryEngine> {
    apply_context_cache(
        MemoryEngine::new(
            Arc::new(MockFactStore::new()),
            Arc::new(MockVectorStore::new()),
            create_llm_provider(settings)?,
            create_embedding_provider(settings)?,
        )
        .with_pii_redaction(settings.enable_pii_redaction)
        .with_event_store(Arc::new(MockMemoryEventStore::new()))
        .with_audit_provider_info(
            Some(llm_provider_name(settings)),
            Some(settings.llm_model.clone()),
        ),
        settings,
    )
}

fn context_cache_config_from_settings(settings: &Settings) -> ContextCacheConfig {
    ContextCacheConfig {
        enabled: settings.context_cache_enabled,
        ttl_seconds: settings.context_cache_ttl_seconds,
        max_entries: settings.context_cache_max_entries,
        stampede_protection_enabled: settings.context_cache_stampede_protection_enabled,
        stampede_lock_timeout_seconds: settings.context_cache_lock_timeout_seconds,
        stale_while_revalidate_enabled: settings.context_cache_stale_while_revalidate_enabled,
        stale_ttl_seconds: settings.context_cache_stale_ttl_seconds,
        metrics_enabled: settings.context_cache_metrics_enabled,
    }
}

fn apply_context_cache(engine: MemoryEngine, settings: &Settings) -> MemcoreResult<MemoryEngine> {
    if settings.context_cache_backend == ContextCacheBackend::Redis {
        return Err(MemcoreError::ValidationError(
            "Redis context cache backend requires async AppState::initialize".to_string(),
        ));
    }

    let config = context_cache_config_from_settings(settings);
    config.validate()?;

    Ok(engine.with_context_cache(
        Arc::new(InMemoryContextCache::new(settings.context_cache_max_entries)),
        config,
    ))
}

async fn apply_context_cache_async(
    engine: MemoryEngine,
    settings: &Settings,
) -> MemcoreResult<MemoryEngine> {
    if settings.context_cache_backend != ContextCacheBackend::Redis {
        return apply_context_cache(engine, settings);
    }

    let config = context_cache_config_from_settings(settings);
    config.validate()?;

    #[cfg(feature = "redis-cache")]
    {
        let redis_url = require_redis_url(settings)?;
        let cache = RedisContextCache::connect(
            &redis_url,
            settings.context_cache_key_prefix.clone(),
            settings.context_cache_ttl_seconds,
        )
        .await?;
        return Ok(engine.with_context_cache(Arc::new(cache), config));
    }

    #[cfg(not(feature = "redis-cache"))]
    {
        Err(MemcoreError::ValidationError(
            "Redis context cache backend requires the `redis-cache` cargo feature".to_string(),
        ))
    }
}

#[cfg(feature = "redis-cache")]
fn require_redis_url(settings: &Settings) -> MemcoreResult<String> {
    settings
        .redis_url
        .as_ref()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            MemcoreError::ValidationError(
                "MEMCORE_REDIS_URL is required when MEMCORE_CONTEXT_CACHE_BACKEND=redis"
                    .to_string(),
            )
        })
}

fn llm_provider_name(settings: &Settings) -> String {
    match settings.llm_provider {
        LlmProviderKind::Mock => "mock".to_string(),
        LlmProviderKind::OpenAi => "openai".to_string(),
        LlmProviderKind::OpenRouter => "openrouter".to_string(),
        LlmProviderKind::Anthropic => "anthropic".to_string(),
        LlmProviderKind::Groq => "groq".to_string(),
    }
}
