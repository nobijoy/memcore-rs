use std::sync::Arc;

use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_config::{FactBackend, Settings};
use memcore_core::{EmbeddingProvider, FactStore, LlmProvider, MemoryEngine, VectorStore};
use memcore_providers::{MockEmbeddingProvider, MockLlmProvider};
use memcore_storage::{MockFactStore, MockVectorStore, SqliteFactStore};

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub started_at: DateTime<Utc>,
    pub memory_engine: Arc<MemoryEngine>,
}

impl AppState {
    /// Builds application state using configured storage and mock vector/providers.
    pub async fn initialize(settings: Settings) -> MemcoreResult<Self> {
        let memory_engine = Arc::new(create_memory_engine(&settings).await?);
        Ok(Self {
            settings,
            started_at: Utc::now(),
            memory_engine,
        })
    }

    /// Synchronous helper for tests and mock-only bootstrap.
    ///
    /// Uses in-memory mocks immediately when `fact_backend` is `Mock`. For SQLite or other
    /// backends, call [`Self::initialize`] from async startup code instead.
    pub fn new(settings: Settings) -> Self {
        if settings.fact_backend == FactBackend::Mock {
            Self {
                started_at: Utc::now(),
                memory_engine: Arc::new(create_mock_memory_engine(&settings)),
                settings,
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
            settings,
            started_at: Utc::now(),
            memory_engine,
        }
    }
}

/// Wires `MemoryEngine` from settings: configurable `FactStore`, mock vector and providers.
pub async fn create_memory_engine(settings: &Settings) -> MemcoreResult<MemoryEngine> {
    let fact_store = create_fact_store(settings).await?;
    let vector_store: Arc<dyn VectorStore> = Arc::new(MockVectorStore::new());
    let llm_provider: Arc<dyn LlmProvider> = Arc::new(MockLlmProvider::new());
    let embedding_provider: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbeddingProvider::new(4));

    Ok(MemoryEngine::new(fact_store, vector_store, llm_provider, embedding_provider)
        .with_pii_redaction(settings.enable_pii_redaction))
}

async fn create_fact_store(settings: &Settings) -> MemcoreResult<Arc<dyn FactStore>> {
    match settings.fact_backend {
        FactBackend::Mock => Ok(Arc::new(MockFactStore::new())),
        FactBackend::Sqlite => {
            let store = SqliteFactStore::connect(&settings.database_url).await?;
            Ok(Arc::new(store))
        }
        FactBackend::Postgres => Err(MemcoreError::ValidationError(
            "postgres fact backend is not wired into the API yet".to_string(),
        )),
    }
}

/// Development wiring: in-memory mock fact store (alias for explicit mock bootstrap).
pub fn create_mock_memory_engine(settings: &Settings) -> MemoryEngine {
    MemoryEngine::new(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        Arc::new(MockLlmProvider::new()),
        Arc::new(MockEmbeddingProvider::new(4)),
    )
    .with_pii_redaction(settings.enable_pii_redaction)
}
