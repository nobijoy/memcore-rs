pub mod context_cache;
#[cfg(feature = "lancedb")]
pub mod lancedb;
pub mod provider_usage;
#[cfg(feature = "qdrant")]
pub mod qdrant;
pub mod keyword_search;
pub mod mocks;
pub mod pagination;
pub mod queries;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod traits;
pub mod vector;

#[cfg(feature = "lancedb")]
pub use lancedb::LanceDbVectorStore;
#[cfg(feature = "qdrant")]
pub use qdrant::QdrantVectorStore;
pub use context_cache::{
    redis_context_cache_key, redis_context_index_key, sanitize_redis_url_for_display,
};
#[cfg(feature = "redis-cache")]
pub use context_cache::RedisContextCache;
pub use mocks::{MockApiKeyStore, MockFactStore, MockMemoryEventStore, MockVectorStore};
pub use provider_usage::MockProviderUsageStore;
pub use queries::{FactSearchQuery, MemoryEventQuery};
#[cfg(feature = "sqlite")]
pub use provider_usage::SqliteProviderUsageStore;
#[cfg(feature = "postgres")]
pub use provider_usage::PostgresProviderUsageStore;
#[cfg(feature = "postgres")]
pub use postgres::{PostgresApiKeyStore, PostgresFactStore, PostgresMemoryEventStore};
#[cfg(feature = "sqlite")]
pub use sqlite::{SqliteApiKeyStore, SqliteFactStore, SqliteMemoryEventStore};
pub use traits::{ApiKeyStore, FactStore, MemoryEventStore, ProviderUsageStore, VectorStore};
pub use vector::{VectorRecord, VectorSearchQuery, VectorSearchResult};
