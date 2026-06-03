use async_trait::async_trait;
use chrono::Utc;
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{Fact, TenantContext};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::{QueryBuilder, Row, Sqlite};
use uuid::Uuid;

use memcore_core::ports::FactSearchQuery;
use crate::sqlite::conversions::{
    datetime_to_str, memory_source_to_str, memory_type_to_str, metadata_to_str,
    optional_datetime_to_str, row_to_fact,
};
use memcore_core::ports::FactStore;

fn storage_error(context: impl Into<String>, error: impl std::fmt::Display) -> MemcoreError {
    MemcoreError::StorageError(format!("{}: {error}", context.into()))
}

fn normalize_sqlite_url(database_url: &str) -> String {
    if let Some(rest) = database_url.strip_prefix("sqlite://") {
        format!("sqlite:{rest}")
    } else {
        database_url.to_string()
    }
}

fn ensure_fact_tenant(fact: &Fact, tenant: &TenantContext) -> MemcoreResult<()> {
    if fact.org_id == tenant.org_id && fact.user_id == tenant.user_id {
        Ok(())
    } else {
        Err(MemcoreError::Forbidden)
    }
}

fn parse_row(row: &sqlx::sqlite::SqliteRow) -> MemcoreResult<Fact> {
    row_to_fact(
        row.try_get("id")
            .map_err(|error| storage_error("row id", error))?,
        row.try_get("org_id")
            .map_err(|error| storage_error("row org_id", error))?,
        row.try_get("user_id")
            .map_err(|error| storage_error("row user_id", error))?,
        row.try_get("memory_type")
            .map_err(|error| storage_error("row memory_type", error))?,
        row.try_get("content")
            .map_err(|error| storage_error("row content", error))?,
        row.try_get("summary")
            .map_err(|error| storage_error("row summary", error))?,
        row.try_get("source")
            .map_err(|error| storage_error("row source", error))?,
        row.try_get("confidence")
            .map_err(|error| storage_error("row confidence", error))?,
        row.try_get("importance")
            .map_err(|error| storage_error("row importance", error))?,
        row.try_get("valid_at")
            .map_err(|error| storage_error("row valid_at", error))?,
        row.try_get("invalid_at")
            .map_err(|error| storage_error("row invalid_at", error))?,
        row.try_get("recorded_at")
            .map_err(|error| storage_error("row recorded_at", error))?,
        row.try_get("updated_at")
            .map_err(|error| storage_error("row updated_at", error))?,
        row.try_get("metadata")
            .map_err(|error| storage_error("row metadata", error))?,
    )
}

#[derive(Clone, Debug)]
pub struct SqliteFactStore {
    pool: SqlitePool,
}

impl SqliteFactStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn connect(database_url: &str) -> MemcoreResult<Self> {
        let url = normalize_sqlite_url(database_url);
        let is_memory = url.contains(":memory:");

        let pool = if is_memory {
            SqlitePoolOptions::new()
                .max_connections(1)
                .min_connections(1)
                .idle_timeout(None)
                .max_lifetime(None)
                .connect(&url)
                .await
        } else {
            SqlitePool::connect(&url).await
        }
        .map_err(|error| storage_error("failed to connect sqlite database", error))?;

        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .map_err(|error| storage_error("failed to run sqlite migrations", error))?;

        Ok(Self::new(pool))
    }

    async fn fetch_fact_row(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
        include_deleted: bool,
    ) -> MemcoreResult<Option<Fact>> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, org_id, user_id, memory_type, content, summary, source, confidence, importance, valid_at, invalid_at, recorded_at, updated_at, metadata FROM facts WHERE id = ",
        );
        query.push_bind(fact_id.to_string());
        query.push(" AND org_id = ");
        query.push_bind(&tenant.org_id);
        query.push(" AND user_id = ");
        query.push_bind(&tenant.user_id);

        if !include_deleted {
            query.push(" AND deleted_at IS NULL");
        }

        let row = query
            .build()
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| storage_error("failed to fetch fact", error))?;

        row.as_ref().map(parse_row).transpose()
    }
}

#[async_trait]
impl FactStore for SqliteFactStore {
    async fn insert_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact> {
        ensure_fact_tenant(&fact, tenant)?;

        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(1) FROM facts WHERE id = ? AND org_id = ? AND user_id = ?",
        )
        .bind(fact.id.to_string())
        .bind(&tenant.org_id)
        .bind(&tenant.user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| storage_error("failed to check existing fact", error))?;

        if exists > 0 {
            return Err(MemcoreError::Conflict(format!(
                "fact already exists: {}",
                fact.id
            )));
        }

        sqlx::query(
            r#"
            INSERT INTO facts (
                id, org_id, user_id, memory_type, content, summary, source,
                confidence, importance, valid_at, invalid_at, recorded_at, updated_at,
                metadata, deleted_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
            "#,
        )
        .bind(fact.id.to_string())
        .bind(&fact.org_id)
        .bind(&fact.user_id)
        .bind(memory_type_to_str(fact.memory_type))
        .bind(&fact.content)
        .bind(&fact.summary)
        .bind(memory_source_to_str(fact.source))
        .bind(fact.confidence)
        .bind(fact.importance)
        .bind(optional_datetime_to_str(fact.valid_at))
        .bind(optional_datetime_to_str(fact.invalid_at))
        .bind(datetime_to_str(fact.recorded_at))
        .bind(datetime_to_str(fact.updated_at))
        .bind(metadata_to_str(&fact.metadata)?)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("failed to insert fact", error))?;

        Ok(fact)
    }

    async fn update_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact> {
        ensure_fact_tenant(&fact, tenant)?;

        let result = sqlx::query(
            r#"
            UPDATE facts SET
                memory_type = ?,
                content = ?,
                summary = ?,
                source = ?,
                confidence = ?,
                importance = ?,
                valid_at = ?,
                invalid_at = ?,
                recorded_at = ?,
                updated_at = ?,
                metadata = ?
            WHERE id = ? AND org_id = ? AND user_id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(memory_type_to_str(fact.memory_type))
        .bind(&fact.content)
        .bind(&fact.summary)
        .bind(memory_source_to_str(fact.source))
        .bind(fact.confidence)
        .bind(fact.importance)
        .bind(optional_datetime_to_str(fact.valid_at))
        .bind(optional_datetime_to_str(fact.invalid_at))
        .bind(datetime_to_str(fact.recorded_at))
        .bind(datetime_to_str(fact.updated_at))
        .bind(metadata_to_str(&fact.metadata)?)
        .bind(fact.id.to_string())
        .bind(&tenant.org_id)
        .bind(&tenant.user_id)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("failed to update fact", error))?;

        if result.rows_affected() == 0 {
            return Err(MemcoreError::NotFound(format!("fact not found: {}", fact.id)));
        }

        Ok(fact)
    }

    async fn get_fact(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<Option<Fact>> {
        self.fetch_fact_row(tenant, fact_id, false).await
    }

    async fn search_facts(&self, query: FactSearchQuery) -> MemcoreResult<Vec<Fact>> {
        // Known issue: `FactSearchQuery.cursor` is intentionally ignored in this phase.
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, org_id, user_id, memory_type, content, summary, source, confidence, importance, valid_at, invalid_at, recorded_at, updated_at, metadata FROM facts WHERE org_id = ",
        );
        builder.push_bind(&query.tenant.org_id);
        builder.push(" AND user_id = ");
        builder.push_bind(&query.tenant.user_id);

        if !query.include_deleted {
            builder.push(" AND deleted_at IS NULL");
        }

        if let Some(memory_types) = &query.memory_types {
            if !memory_types.is_empty() {
                builder.push(" AND memory_type IN (");
                let mut separated = builder.separated(", ");
                for memory_type in memory_types {
                    separated.push_bind(memory_type_to_str(*memory_type));
                }
                separated.push_unseparated(") ");
            }
        }

        if let Some(query_text) = &query.query_text {
            let pattern = format!("%{}%", query_text.to_ascii_lowercase());
            builder.push(" AND LOWER(content) LIKE ");
            builder.push_bind(pattern);
        }

        builder.push(" ORDER BY updated_at DESC LIMIT ");
        builder.push_bind(query.limit as i64);

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| storage_error("failed to search facts", error))?;

        rows.iter().map(parse_row).collect()
    }

    async fn soft_delete_fact(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<()> {
        let deleted_at = datetime_to_str(Utc::now());
        let result = sqlx::query(
            "UPDATE facts SET deleted_at = ? WHERE id = ? AND org_id = ? AND user_id = ? AND deleted_at IS NULL",
        )
        .bind(deleted_at)
        .bind(fact_id.to_string())
        .bind(&tenant.org_id)
        .bind(&tenant.user_id)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("failed to soft delete fact", error))?;

        if result.rows_affected() == 0 {
            return Err(MemcoreError::NotFound(format!("fact not found: {fact_id}")));
        }

        Ok(())
    }

    async fn delete_user_data(&self, tenant: &TenantContext) -> MemcoreResult<()> {
        sqlx::query("DELETE FROM facts WHERE org_id = ? AND user_id = ?")
            .bind(&tenant.org_id)
            .bind(&tenant.user_id)
            .execute(&self.pool)
            .await
            .map_err(|error| storage_error("failed to delete user data", error))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use memcore_core::{MemorySource, MemoryType, TenantContext};
    use serde_json::json;
    use uuid::Uuid;

    use super::SqliteFactStore;
    use memcore_core::ports::{FactSearchQuery, FactStore};

    async fn test_store() -> SqliteFactStore {
        SqliteFactStore::connect("sqlite::memory:?cache=shared")
            .await
            .expect("sqlite store should connect")
    }

    fn tenant(org_id: &str, user_id: &str) -> TenantContext {
        TenantContext::new(org_id, user_id).expect("tenant should be valid")
    }

    fn sample_fact(
        org_id: &str,
        user_id: &str,
        content: &str,
        memory_type: MemoryType,
        metadata: serde_json::Value,
        valid_at: Option<chrono::DateTime<Utc>>,
        invalid_at: Option<chrono::DateTime<Utc>>,
    ) -> memcore_core::Fact {
        let now = Utc::now();
        memcore_core::Fact::new(
            Uuid::new_v4(),
            org_id,
            user_id,
            memory_type,
            content,
            Some("summary".to_string()),
            MemorySource::UserMessage,
            0.9,
            0.8,
            valid_at,
            invalid_at,
            now,
            now,
            metadata,
        )
        .expect("fact should be valid")
    }

    #[tokio::test]
    async fn insert_and_get_fact() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let fact = sample_fact(
            "org_a",
            "user_a",
            "learning rust",
            MemoryType::Skill,
            json!({}),
            None,
            None,
        );

        store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");

        let fetched = store
            .get_fact(&tenant, fact.id)
            .await
            .expect("get should succeed")
            .expect("fact should exist");

        assert_eq!(fetched.content, "learning rust");
    }

    #[tokio::test]
    async fn update_fact() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let mut fact = sample_fact(
            "org_a",
            "user_a",
            "original",
            MemoryType::Profile,
            json!({}),
            None,
            None,
        );

        store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");

        fact.content = "updated content".to_string();
        store
            .update_fact(&tenant, fact.clone())
            .await
            .expect("update should succeed");

        let fetched = store
            .get_fact(&tenant, fact.id)
            .await
            .expect("get should succeed")
            .expect("fact should exist");
        assert_eq!(fetched.content, "updated content");
    }

    #[tokio::test]
    async fn search_facts_by_tenant() {
        let store = test_store().await;
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_b", "user_b");

        store
            .insert_fact(
                &tenant_a,
                sample_fact(
                    "org_a",
                    "user_a",
                    "rust backend",
                    MemoryType::Skill,
                    json!({}),
                    None,
                    None,
                ),
            )
            .await
            .expect("insert should succeed");
        store
            .insert_fact(
                &tenant_b,
                sample_fact(
                    "org_b",
                    "user_b",
                    "rust backend",
                    MemoryType::Skill,
                    json!({}),
                    None,
                    None,
                ),
            )
            .await
            .expect("insert should succeed");

        let results = store
            .search_facts(FactSearchQuery {
                tenant: tenant_a,
                memory_types: None,
                query_text: Some("rust".to_string()),
                limit: 10,
                cursor: None,
                include_deleted: false,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].org_id, "org_a");
    }

    #[tokio::test]
    async fn search_facts_by_memory_type() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");

        store
            .insert_fact(
                &tenant,
                sample_fact(
                    "org_a",
                    "user_a",
                    "skill fact",
                    MemoryType::Skill,
                    json!({}),
                    None,
                    None,
                ),
            )
            .await
            .expect("insert should succeed");
        store
            .insert_fact(
                &tenant,
                sample_fact(
                    "org_a",
                    "user_a",
                    "profile fact",
                    MemoryType::Profile,
                    json!({}),
                    None,
                    None,
                ),
            )
            .await
            .expect("insert should succeed");

        let results = store
            .search_facts(FactSearchQuery {
                tenant,
                memory_types: Some(vec![MemoryType::Profile]),
                query_text: None,
                limit: 10,
                cursor: None,
                include_deleted: false,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory_type, MemoryType::Profile);
    }

    #[tokio::test]
    async fn tenant_isolation_prevents_cross_tenant_reads() {
        let store = test_store().await;
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");
        let fact = sample_fact(
            "org_a",
            "user_a",
            "private",
            MemoryType::Profile,
            json!({}),
            None,
            None,
        );

        store
            .insert_fact(&tenant_a, fact.clone())
            .await
            .expect("insert should succeed");

        let cross = store
            .get_fact(&tenant_b, fact.id)
            .await
            .expect("get should succeed");
        assert!(cross.is_none());
    }

    #[tokio::test]
    async fn soft_delete_hides_fact_from_normal_search() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let fact = sample_fact(
            "org_a",
            "user_a",
            "delete me",
            MemoryType::Task,
            json!({}),
            None,
            None,
        );

        store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");
        store
            .soft_delete_fact(&tenant, fact.id)
            .await
            .expect("soft delete should succeed");

        assert!(store
            .get_fact(&tenant, fact.id)
            .await
            .expect("get should succeed")
            .is_none());

        let results = store
            .search_facts(FactSearchQuery::new(tenant.clone(), 10))
            .await
            .expect("search should succeed");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn include_deleted_returns_soft_deleted_fact() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let fact = sample_fact(
            "org_a",
            "user_a",
            "deleted fact",
            MemoryType::Task,
            json!({}),
            None,
            None,
        );

        store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");
        store
            .soft_delete_fact(&tenant, fact.id)
            .await
            .expect("soft delete should succeed");

        let results = store
            .search_facts(FactSearchQuery {
                tenant,
                memory_types: None,
                query_text: None,
                limit: 10,
                cursor: None,
                include_deleted: true,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, fact.id);
    }

    #[tokio::test]
    async fn delete_user_data_removes_only_target_user() {
        let store = test_store().await;
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");

        store
            .insert_fact(
                &tenant_a,
                sample_fact(
                    "org_a",
                    "user_a",
                    "user a",
                    MemoryType::Profile,
                    json!({}),
                    None,
                    None,
                ),
            )
            .await
            .expect("insert should succeed");
        store
            .insert_fact(
                &tenant_b,
                sample_fact(
                    "org_a",
                    "user_b",
                    "user b",
                    MemoryType::Profile,
                    json!({}),
                    None,
                    None,
                ),
            )
            .await
            .expect("insert should succeed");

        store
            .delete_user_data(&tenant_a)
            .await
            .expect("delete should succeed");

        assert!(store
            .search_facts(FactSearchQuery::new(tenant_a, 10))
            .await
            .expect("search")
            .is_empty());
        assert_eq!(
            store
                .search_facts(FactSearchQuery::new(tenant_b, 10))
                .await
                .expect("search")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn metadata_round_trip_works() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let fact = sample_fact(
            "org_a",
            "user_a",
            "metadata test",
            MemoryType::System,
            json!({ "topic": "rust", "level": 2 }),
            None,
            None,
        );

        store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");

        let fetched = store
            .get_fact(&tenant, fact.id)
            .await
            .expect("get should succeed")
            .expect("fact should exist");
        assert_eq!(fetched.metadata["topic"], "rust");
        assert_eq!(fetched.metadata["level"], 2);
    }

    #[tokio::test]
    async fn valid_and_invalid_at_round_trip_works() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let valid_at = Utc::now() - Duration::days(2);
        let invalid_at = Utc::now() - Duration::days(1);
        let fact = sample_fact(
            "org_a",
            "user_a",
            "temporal fact",
            MemoryType::Entity,
            json!({}),
            Some(valid_at),
            Some(invalid_at),
        );

        store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");

        let fetched = store
            .get_fact(&tenant, fact.id)
            .await
            .expect("get should succeed")
            .expect("fact should exist");

        assert_eq!(fetched.valid_at, Some(valid_at));
        assert_eq!(fetched.invalid_at, Some(invalid_at));
    }
}
