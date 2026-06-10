use std::sync::Arc;

use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_config::{
    EmbeddingProviderKind, EventBackend, FactBackend, LlmProviderKind, Settings, VectorBackend,
};
use memcore_core::{EmbeddingProvider, FactStore, LlmProvider, MemoryEngine, MemoryEventStore, VectorStore};
use memcore_providers::{
    MockEmbeddingProvider, MockLlmProvider, OpenAiClient, OpenAiEmbeddingProvider,
    OpenAiLlmProvider, default_embedding_dimensions_for_model,
};
use memcore_storage::{
    MockFactStore, MockMemoryEventStore, MockVectorStore, SqliteFactStore,
    SqliteMemoryEventStore,
};
#[cfg(feature = "postgres")]
use memcore_storage::{PostgresFactStore, PostgresMemoryEventStore};
use crate::middleware::RateLimiter;
use crate::observability::Metrics;
#[cfg(feature = "lancedb")]
use memcore_storage::LanceDbVectorStore;

/// Default embedding dimensions for the mock embedding provider.
const MOCK_EMBEDDING_DIMENSIONS: usize = 4;

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub started_at: DateTime<Utc>,
    pub memory_engine: Arc<MemoryEngine>,
    pub rate_limiter: Arc<RateLimiter>,
    pub metrics: Arc<Metrics>,
}

impl AppState {
    /// Builds application state using configured storage and providers.
    pub async fn initialize(settings: Settings) -> MemcoreResult<Self> {
        let memory_engine = Arc::new(create_memory_engine(&settings).await?);
        Ok(Self {
            settings: settings.clone(),
            started_at: Utc::now(),
            memory_engine,
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
                memory_engine: Arc::new(create_mock_memory_engine(&settings)),
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

    Ok(MemoryEngine::new(
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
    ))
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

fn create_llm_provider(settings: &Settings) -> MemcoreResult<Arc<dyn LlmProvider>> {
    match settings.llm_provider {
        LlmProviderKind::Mock => Ok(Arc::new(MockLlmProvider::new())),
        LlmProviderKind::OpenAi => {
            let api_key = require_openai_api_key(
                settings,
                "OPENAI_API_KEY is required when MEMCORE_LLM_PROVIDER=openai",
            )?;
            let client = OpenAiClient::new(&api_key, &settings.openai_base_url).map_err(|err| {
                provider_init_error("OpenAI LLM", err)
            })?;
            Ok(Arc::new(OpenAiLlmProvider::new(client, settings.llm_model.clone())))
        }
        LlmProviderKind::OpenRouter => Err(MemcoreError::ValidationError(
            "openrouter LLM provider is not wired into the API yet".to_string(),
        )),
        LlmProviderKind::Anthropic => Err(MemcoreError::ValidationError(
            "anthropic LLM provider is not wired into the API yet".to_string(),
        )),
        LlmProviderKind::Groq => Err(MemcoreError::ValidationError(
            "groq LLM provider is not wired into the API yet".to_string(),
        )),
    }
}

fn create_embedding_provider(settings: &Settings) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    match settings.embedding_provider {
        EmbeddingProviderKind::Mock => {
            Ok(Arc::new(MockEmbeddingProvider::new(MOCK_EMBEDDING_DIMENSIONS)))
        }
        EmbeddingProviderKind::OpenAi => {
            let api_key = require_openai_api_key(
                settings,
                "OPENAI_API_KEY is required when MEMCORE_EMBEDDING_PROVIDER=openai",
            )?;
            let client = OpenAiClient::new(&api_key, &settings.openai_base_url).map_err(|err| {
                provider_init_error("OpenAI embedding", err)
            })?;
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
    }
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
        VectorBackend::Qdrant => Err(MemcoreError::ValidationError(
            "qdrant vector backend is not wired into the API yet".to_string(),
        )),
    }
}

/// In-memory mock fact and vector stores for fast API tests.
pub fn create_mock_memory_engine(settings: &Settings) -> MemoryEngine {
    MemoryEngine::new(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        Arc::new(MockLlmProvider::new()),
        Arc::new(MockEmbeddingProvider::new(MOCK_EMBEDDING_DIMENSIONS)),
    )
    .with_pii_redaction(settings.enable_pii_redaction)
    .with_event_store(Arc::new(MockMemoryEventStore::new()))
    .with_audit_provider_info(
        Some(llm_provider_name(settings)),
        Some(settings.llm_model.clone()),
    )
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
