use std::sync::Arc;

use chrono::{DateTime, Utc};
use memcore_config::Settings;
use memcore_core::MemoryEngine;
use memcore_providers::{MockEmbeddingProvider, MockLlmProvider};
use memcore_storage::{MockFactStore, MockVectorStore};

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub started_at: DateTime<Utc>,
    pub memory_engine: Arc<MemoryEngine>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            started_at: Utc::now(),
            memory_engine: Arc::new(create_mock_memory_engine()),
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

/// Development wiring: in-memory mock storage and providers until real backends are configured.
pub fn create_mock_memory_engine() -> MemoryEngine {
    MemoryEngine::new(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        Arc::new(MockLlmProvider::new()),
        Arc::new(MockEmbeddingProvider::new(4)),
    )
}
