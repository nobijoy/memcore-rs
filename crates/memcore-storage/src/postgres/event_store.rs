use async_trait::async_trait;
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{MemoryEvent, TenantContext};
use sqlx::postgres::PgPool;
use sqlx::{Postgres, QueryBuilder, Row};

use memcore_core::ports::{
    MemoryEventQuery, MemoryEventStore, DEFAULT_MEMORY_EVENT_LIST_LIMIT,
    MAX_MEMORY_EVENT_LIST_LIMIT,
};

use super::conversions::{memory_event_operation_to_str, row_to_memory_event};

fn storage_error(context: impl Into<String>, error: impl std::fmt::Display) -> MemcoreError {
    MemcoreError::StorageError(format!("{}: {error}", context.into()))
}

fn ensure_event_tenant(event: &MemoryEvent, tenant: &TenantContext) -> MemcoreResult<()> {
    if event.org_id == tenant.org_id && event.user_id == tenant.user_id {
        Ok(())
    } else {
        Err(MemcoreError::Forbidden)
    }
}

fn normalize_event_list_limit(limit: usize) -> MemcoreResult<usize> {
    if limit == 0 {
        return Ok(DEFAULT_MEMORY_EVENT_LIST_LIMIT);
    }

    if limit > MAX_MEMORY_EVENT_LIST_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_MEMORY_EVENT_LIST_LIMIT}"
        )));
    }

    Ok(limit)
}

fn parse_event_row(row: &sqlx::postgres::PgRow) -> MemcoreResult<MemoryEvent> {
    row_to_memory_event(
        row.try_get("id")
            .map_err(|error| storage_error("row id", error))?,
        row.try_get("org_id")
            .map_err(|error| storage_error("row org_id", error))?,
        row.try_get("user_id")
            .map_err(|error| storage_error("row user_id", error))?,
        row.try_get("fact_id")
            .map_err(|error| storage_error("row fact_id", error))?,
        row.try_get("operation")
            .map_err(|error| storage_error("row operation", error))?,
        row.try_get("input_text")
            .map_err(|error| storage_error("row input_text", error))?,
        row.try_get("previous_content")
            .map_err(|error| storage_error("row previous_content", error))?,
        row.try_get("new_content")
            .map_err(|error| storage_error("row new_content", error))?,
        row.try_get("provider_name")
            .map_err(|error| storage_error("row provider_name", error))?,
        row.try_get("model_name")
            .map_err(|error| storage_error("row model_name", error))?,
        row.try_get("metadata")
            .map_err(|error| storage_error("row metadata", error))?,
        row.try_get("created_at")
            .map_err(|error| storage_error("row created_at", error))?,
    )
}

#[derive(Clone, Debug)]
pub struct PostgresMemoryEventStore {
    pool: PgPool,
}

impl PostgresMemoryEventStore {
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
}

#[async_trait]
impl MemoryEventStore for PostgresMemoryEventStore {
    async fn record_event(
        &self,
        tenant: &TenantContext,
        event: MemoryEvent,
    ) -> MemcoreResult<MemoryEvent> {
        ensure_event_tenant(&event, tenant)?;

        sqlx::query(
            r#"
            INSERT INTO memory_events (
                id, org_id, user_id, fact_id, operation,
                input_text, previous_content, new_content,
                provider_name, model_name, metadata, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(event.id)
        .bind(&event.org_id)
        .bind(&event.user_id)
        .bind(event.fact_id)
        .bind(memory_event_operation_to_str(event.operation))
        .bind(&event.input_text)
        .bind(&event.previous_content)
        .bind(&event.new_content)
        .bind(&event.provider_name)
        .bind(&event.model_name)
        .bind(&event.metadata)
        .bind(event.created_at)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("failed to insert memory event", error))?;

        Ok(event)
    }

    async fn list_events(&self, query: MemoryEventQuery) -> MemcoreResult<Vec<MemoryEvent>> {
        let limit = normalize_event_list_limit(query.limit)?;
        let _ = query.cursor;

        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT id, org_id, user_id, fact_id, operation, input_text, previous_content, new_content, provider_name, model_name, metadata, created_at FROM memory_events WHERE org_id = ",
        );
        builder.push_bind(query.tenant.org_id.clone());
        builder.push(" AND user_id = ");
        builder.push_bind(query.tenant.user_id.clone());

        if let Some(fact_id) = query.fact_id {
            builder.push(" AND fact_id = ");
            builder.push_bind(fact_id);
        }

        if let Some(operation) = query.operation {
            builder.push(" AND operation = ");
            builder.push_bind(memory_event_operation_to_str(operation));
        }

        builder.push(" ORDER BY created_at DESC LIMIT ");
        builder.push_bind(i64::try_from(limit).map_err(|error| {
            storage_error("event list limit out of range for postgres", error)
        })?);

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| storage_error("failed to list memory events", error))?;

        rows.iter().map(parse_event_row).collect()
    }
}
