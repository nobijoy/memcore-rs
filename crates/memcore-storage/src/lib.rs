#[cfg(feature = "lancedb")]
pub mod lancedb;
#[cfg(feature = "qdrant")]
pub mod qdrant;
pub mod mocks;
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
pub use mocks::{MockApiKeyStore, MockFactStore, MockMemoryEventStore, MockVectorStore};
pub use queries::{FactSearchQuery, MemoryEventQuery};
#[cfg(feature = "postgres")]
pub use postgres::{PostgresApiKeyStore, PostgresFactStore, PostgresMemoryEventStore};
#[cfg(feature = "sqlite")]
pub use sqlite::{SqliteApiKeyStore, SqliteFactStore, SqliteMemoryEventStore};
pub use traits::{ApiKeyStore, FactStore, MemoryEventStore, VectorStore};
pub use vector::{VectorRecord, VectorSearchQuery, VectorSearchResult};
