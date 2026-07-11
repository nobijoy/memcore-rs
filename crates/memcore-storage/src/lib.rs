pub mod backup;
pub mod context_cache;
pub mod jobs;
pub mod keyword_search;
#[cfg(feature = "lancedb")]
pub mod lancedb;
pub mod memory_usage;
pub mod migrations;
pub mod mocks;
pub mod org_plan;
pub mod pagination;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod provider_usage;
#[cfg(feature = "qdrant")]
pub mod qdrant;
pub mod queries;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod startup_checks;
pub mod traits;
pub mod vector;

pub use backup::DatabaseBackupProvider;
#[cfg(feature = "postgres")]
pub use backup::PostgresBackupProvider;
#[cfg(feature = "sqlite")]
pub use backup::{SqliteBackupProvider, sqlite_path_from_database_url};
#[cfg(feature = "redis-cache")]
pub use context_cache::RedisContextCache;
pub use context_cache::{
    redis_context_cache_key, redis_context_index_key, sanitize_redis_url_for_display,
};
pub use jobs::{MockBackgroundJobLockStore, MockBackgroundJobRunStore};
#[cfg(feature = "postgres")]
pub use jobs::{PostgresBackgroundJobLockStore, PostgresBackgroundJobRunStore};
#[cfg(feature = "sqlite")]
pub use jobs::{SqliteBackgroundJobLockStore, SqliteBackgroundJobRunStore};
#[cfg(feature = "lancedb")]
pub use lancedb::LanceDbVectorStore;
pub use memory_usage::MockMemoryUsageSnapshotStore;
#[cfg(feature = "postgres")]
pub use memory_usage::PostgresMemoryUsageSnapshotStore;
#[cfg(feature = "sqlite")]
pub use memory_usage::SqliteMemoryUsageSnapshotStore;
pub use migrations::{
    AppliedMigration, Migration, MigrationIssue, MigrationRunner, MigrationStatus,
    MigrationValidationReport, migration_checksum,
};
#[cfg(feature = "postgres")]
pub use migrations::{PostgresMigrationRunner, postgres_migrations};
#[cfg(feature = "sqlite")]
pub use migrations::{SqliteMigrationRunner, sqlite_migrations};
pub use mocks::{MockApiKeyStore, MockFactStore, MockMemoryEventStore, MockVectorStore};
pub use org_plan::MockOrgPlanStore;
#[cfg(feature = "postgres")]
pub use org_plan::PostgresOrgPlanStore;
#[cfg(feature = "sqlite")]
pub use org_plan::SqliteOrgPlanStore;
#[cfg(feature = "postgres")]
pub use postgres::{PostgresApiKeyStore, PostgresFactStore, PostgresMemoryEventStore};
pub use provider_usage::MockProviderUsageStore;
#[cfg(feature = "postgres")]
pub use provider_usage::PostgresProviderUsageStore;
#[cfg(feature = "sqlite")]
pub use provider_usage::SqliteProviderUsageStore;
#[cfg(feature = "qdrant")]
pub use qdrant::QdrantVectorStore;
pub use queries::{FactSearchQuery, MemoryEventQuery};
#[cfg(feature = "sqlite")]
pub use sqlite::{SqliteApiKeyStore, SqliteFactStore, SqliteMemoryEventStore};
pub use startup_checks::{StorageMigrationMode, StorageStartupCheckReport};
#[cfg(feature = "postgres")]
pub use startup_checks::{
    check_postgres_pool_startup, check_postgres_startup, connect_postgres_pool,
};
#[cfg(feature = "sqlite")]
pub use startup_checks::{check_sqlite_pool_startup, check_sqlite_startup, connect_sqlite_pool};
pub use traits::{
    ApiKeyStore, BackgroundJobLockStore, BackgroundJobRunStore, FactStore, MemoryEventStore,
    MemoryUsageSnapshotStore, OrgPlanStore, ProviderUsageStore, VectorStore,
};
pub use vector::{VectorRecord, VectorSearchQuery, VectorSearchResult};
