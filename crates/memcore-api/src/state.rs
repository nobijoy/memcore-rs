use std::sync::Arc;

use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_config::{FactBackend, Settings, VectorBackend};
use memcore_core::{EmbeddingProvider, FactStore, LlmProvider, MemoryEngine, VectorStore};
use memcore_providers::{MockEmbeddingProvider, MockLlmProvider};
use memcore_storage::{MockFactStore, MockVectorStore, SqliteFactStore};
#[cfg(feature = "lancedb")]
use memcore_storage::LanceDbVectorStore;

/// Default embedding dimensions for the mock embedding provider until config exposes this.
const DEFAULT_EMBEDDING_DIMENSIONS: usize = 4;

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub started_at: DateTime<Utc>,
    pub memory_engine: Arc<MemoryEngine>,
}

impl AppState {
    /// Builds application state using configured storage and mock providers.
    pub async fn initialize(settings: Settings) -> MemcoreResult<Self> {
        let memory_engine = Arc::new(create_memory_engine(&settings).await?);
        Ok(Self {
            settings,
            started_at: Utc::now(),
            memory_engine,
        })
    }

    /// Synchronous helper for tests when both fact and vector backends are mock.
    ///
    /// For SQLite or LanceDB backends, call [`Self::initialize`] from async code instead.
    pub fn new(settings: Settings) -> Self {
        if settings.fact_backend == FactBackend::Mock
            && settings.vector_backend == VectorBackend::Mock
        {
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

/// Wires `MemoryEngine` from settings: configurable fact/vector stores, mock LLM/embedding.
pub async fn create_memory_engine(settings: &Settings) -> MemcoreResult<MemoryEngine> {
    let fact_store = create_fact_store(settings).await?;
    let llm_provider: Arc<dyn LlmProvider> = Arc::new(MockLlmProvider::new());
    let embedding_provider: Arc<dyn EmbeddingProvider> =
        Arc::new(MockEmbeddingProvider::new(DEFAULT_EMBEDDING_DIMENSIONS));
    let vector_store =
        create_vector_store(settings, embedding_provider.dimensions()).await?;

    Ok(MemoryEngine::new(
        fact_store,
        vector_store,
        llm_provider,
        embedding_provider,
    )
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
        Arc::new(MockEmbeddingProvider::new(DEFAULT_EMBEDDING_DIMENSIONS)),
    )
    .with_pii_redaction(settings.enable_pii_redaction)
}
