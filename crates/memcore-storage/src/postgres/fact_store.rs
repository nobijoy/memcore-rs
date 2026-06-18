use async_trait::async_trait;
use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{Fact, TenantContext};
use sqlx::postgres::PgPool;
use sqlx::{Postgres, QueryBuilder, Row};
use uuid::Uuid;

use memcore_core::ports::{FactSearchQuery, FactStore, RetentionPruneResult};

use super::conversions::{memory_source_to_str, memory_type_to_str, row_to_fact};

fn storage_error(context: impl Into<String>, error: impl std::fmt::Display) -> MemcoreError {
    MemcoreError::StorageError(format!("{}: {error}", context.into()))
}

fn ensure_fact_tenant(fact: &Fact, tenant: &TenantContext) -> MemcoreResult<()> {
    if fact.org_id == tenant.org_id && fact.user_id == tenant.user_id {
        Ok(())
    } else {
        Err(MemcoreError::Forbidden)
    }
}

fn parse_row(row: &sqlx::postgres::PgRow) -> MemcoreResult<Fact> {
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
pub struct PostgresFactStore {
    pool: PgPool,
}

impl PostgresFactStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn connect(database_url: &str) -> MemcoreResult<Self> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|error| storage_error("failed to connect postgres database", error))?;

        sqlx::migrate!("./migrations/postgres")
            .run(&pool)
            .await
            .map_err(|error| storage_error("failed to run postgres migrations", error))?;

        Ok(Self::new(pool))
    }

    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    async fn fetch_fact_row(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
        include_deleted: bool,
    ) -> MemcoreResult<Option<Fact>> {
        let mut query = QueryBuilder::<Postgres>::new(
            "SELECT id, org_id, user_id, memory_type, content, summary, source, confidence, importance, valid_at, invalid_at, recorded_at, updated_at, metadata FROM facts WHERE id = ",
        );
        query.push_bind(fact_id);
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
impl FactStore for PostgresFactStore {
    async fn insert_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact> {
        ensure_fact_tenant(&fact, tenant)?;

        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(1) FROM facts WHERE id = $1 AND org_id = $2 AND user_id = $3",
        )
        .bind(fact.id)
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, NULL)
            "#,
        )
        .bind(fact.id)
        .bind(&fact.org_id)
        .bind(&fact.user_id)
        .bind(memory_type_to_str(fact.memory_type))
        .bind(&fact.content)
        .bind(&fact.summary)
        .bind(memory_source_to_str(fact.source))
        .bind(fact.confidence)
        .bind(fact.importance)
        .bind(fact.valid_at)
        .bind(fact.invalid_at)
        .bind(fact.recorded_at)
        .bind(fact.updated_at)
        .bind(&fact.metadata)
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
                memory_type = $1,
                content = $2,
                summary = $3,
                source = $4,
                confidence = $5,
                importance = $6,
                valid_at = $7,
                invalid_at = $8,
                recorded_at = $9,
                updated_at = $10,
                metadata = $11
            WHERE id = $12 AND org_id = $13 AND user_id = $14 AND deleted_at IS NULL
            "#,
        )
        .bind(memory_type_to_str(fact.memory_type))
        .bind(&fact.content)
        .bind(&fact.summary)
        .bind(memory_source_to_str(fact.source))
        .bind(fact.confidence)
        .bind(fact.importance)
        .bind(fact.valid_at)
        .bind(fact.invalid_at)
        .bind(fact.recorded_at)
        .bind(fact.updated_at)
        .bind(&fact.metadata)
        .bind(fact.id)
        .bind(&tenant.org_id)
        .bind(&tenant.user_id)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("failed to update fact", error))?;

        if result.rows_affected() == 0 {
            return Err(MemcoreError::NotFound(format!(
                "fact not found: {}",
                fact.id
            )));
        }

        Ok(fact)
    }

    async fn get_fact(&self, tenant: &TenantContext, fact_id: Uuid) -> MemcoreResult<Option<Fact>> {
        self.fetch_fact_row(tenant, fact_id, false).await
    }

    async fn search_facts(&self, query: FactSearchQuery) -> MemcoreResult<Vec<Fact>> {
        use crate::keyword_search::push_postgres_fact_keyword_filter;
        use crate::pagination::{fetch_limit, push_postgres_desc_cursor_uuid};

        let mut builder = QueryBuilder::<Postgres>::new(
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
            push_postgres_fact_keyword_filter(&mut builder, query_text);
        }

        if let Some(cursor) = &query.cursor {
            push_postgres_desc_cursor_uuid(&mut builder, "updated_at", "id", cursor);
        }

        builder.push(" ORDER BY updated_at DESC, id DESC LIMIT ");
        builder.push_bind(i64::try_from(fetch_limit(query.limit)).map_err(|error| {
            storage_error("fact search limit out of range for postgres", error)
        })?);

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| storage_error("failed to search facts", error))?;

        rows.iter().map(parse_row).collect()
    }

    async fn soft_delete_fact(&self, tenant: &TenantContext, fact_id: Uuid) -> MemcoreResult<()> {
        let deleted_at = Utc::now();
        let result = sqlx::query(
            "UPDATE facts SET deleted_at = $1 WHERE id = $2 AND org_id = $3 AND user_id = $4 AND deleted_at IS NULL",
        )
        .bind(deleted_at)
        .bind(fact_id)
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
        sqlx::query("DELETE FROM facts WHERE org_id = $1 AND user_id = $2")
            .bind(&tenant.org_id)
            .bind(&tenant.user_id)
            .execute(&self.pool)
            .await
            .map_err(|error| storage_error("failed to delete user data", error))?;

        Ok(())
    }

    async fn delete_facts_older_than(
        &self,
        tenant: &TenantContext,
        cutoff: DateTime<Utc>,
        dry_run: bool,
    ) -> MemcoreResult<RetentionPruneResult> {
        let rows = sqlx::query(
            "SELECT id FROM facts WHERE org_id = $1 AND user_id = $2 AND deleted_at IS NULL AND updated_at < $3",
        )
        .bind(&tenant.org_id)
        .bind(&tenant.user_id)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| storage_error("failed to list facts for retention", error))?;

        let fact_ids: Vec<Uuid> = rows
            .iter()
            .map(|row| {
                row.try_get("id")
                    .map_err(|error| storage_error("row id", error))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if dry_run {
            return Ok(RetentionPruneResult {
                count: fact_ids.len(),
                fact_ids: Vec::new(),
            });
        }

        if fact_ids.is_empty() {
            return Ok(RetentionPruneResult {
                count: 0,
                fact_ids: Vec::new(),
            });
        }

        let deleted_at = Utc::now();
        let result = sqlx::query(
            "UPDATE facts SET deleted_at = $1 WHERE org_id = $2 AND user_id = $3 AND deleted_at IS NULL AND updated_at < $4",
        )
        .bind(deleted_at)
        .bind(&tenant.org_id)
        .bind(&tenant.user_id)
        .bind(cutoff)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("failed to soft delete facts for retention", error))?;

        Ok(RetentionPruneResult {
            count: result.rows_affected() as usize,
            fact_ids,
        })
    }

    async fn count_facts_by_org(&self, org_id: &str) -> MemcoreResult<usize> {
        let row = sqlx::query(
            "SELECT COUNT(*)::bigint AS count FROM facts WHERE org_id = $1 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| storage_error("failed to count facts by org", error))?;

        let count: i64 = row
            .try_get("count")
            .map_err(|error| storage_error("row count", error))?;
        Ok(count as usize)
    }

    async fn count_facts_by_user(&self, tenant: &TenantContext) -> MemcoreResult<usize> {
        let row = sqlx::query(
            "SELECT COUNT(*)::bigint AS count FROM facts WHERE org_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(&tenant.org_id)
        .bind(&tenant.user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| storage_error("failed to count facts by user", error))?;

        let count: i64 = row
            .try_get("count")
            .map_err(|error| storage_error("row count", error))?;
        Ok(count as usize)
    }

    async fn count_users_by_org(&self, org_id: &str) -> MemcoreResult<usize> {
        let row = sqlx::query(
            "SELECT COUNT(DISTINCT user_id)::bigint AS count FROM facts WHERE org_id = $1 AND deleted_at IS NULL",
        )
        .bind(org_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| storage_error("failed to count users by org", error))?;

        let count: i64 = row
            .try_get("count")
            .map_err(|error| storage_error("row count", error))?;
        Ok(count as usize)
    }

    async fn list_users_by_org(
        &self,
        query: memcore_core::ports::OrgUserListQuery,
    ) -> MemcoreResult<Vec<memcore_core::ports::OrgUserSummary>> {
        use memcore_core::ports::OrgUserSummary;

        use crate::pagination::fetch_limit;

        let fetch = i64::try_from(fetch_limit(query.limit)).map_err(|error| {
            storage_error("org users list limit out of range for postgres", error)
        })?;

        let rows = if let Some(cursor) = &query.cursor {
            sqlx::query(
                r#"
                SELECT user_id, COUNT(*)::bigint AS memory_count, MAX(updated_at) AS last_memory_at
                FROM facts
                WHERE org_id = $1 AND deleted_at IS NULL
                GROUP BY user_id
                HAVING (MAX(updated_at) < $2 OR (MAX(updated_at) = $2 AND user_id < $3))
                ORDER BY MAX(updated_at) DESC, user_id DESC
                LIMIT $4
                "#,
            )
            .bind(&query.org_id)
            .bind(cursor.last_sort_value)
            .bind(cursor.last_sort_value)
            .bind(&cursor.last_id)
            .bind(fetch)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                r#"
                SELECT user_id, COUNT(*)::bigint AS memory_count, MAX(updated_at) AS last_memory_at
                FROM facts
                WHERE org_id = $1 AND deleted_at IS NULL
                GROUP BY user_id
                ORDER BY MAX(updated_at) DESC, user_id DESC
                LIMIT $2
                "#,
            )
            .bind(&query.org_id)
            .bind(fetch)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|error| storage_error("failed to list users by org", error))?;

        rows.iter()
            .map(|row| {
                let user_id: String = row
                    .try_get("user_id")
                    .map_err(|error| storage_error("row user_id", error))?;
                let memory_count: i64 = row
                    .try_get("memory_count")
                    .map_err(|error| storage_error("row memory_count", error))?;
                let last_memory_at: Option<DateTime<Utc>> = row
                    .try_get("last_memory_at")
                    .map_err(|error| storage_error("row last_memory_at", error))?;

                Ok(OrgUserSummary {
                    user_id,
                    memory_count: memory_count as usize,
                    last_memory_at,
                })
            })
            .collect()
    }
}
