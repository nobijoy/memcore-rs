use async_trait::async_trait;
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{MemoryEvent, TenantContext};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::{QueryBuilder, Row, Sqlite};

use memcore_core::ports::{
    MemoryEventQuery, MemoryEventStore, DEFAULT_MEMORY_EVENT_LIST_LIMIT,
    MAX_MEMORY_EVENT_LIST_LIMIT,
};
use crate::sqlite::conversions::{
    datetime_to_str, memory_event_operation_to_str, metadata_to_str, optional_uuid_to_str,
    row_to_memory_event,
};

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

fn parse_event_row(row: &sqlx::sqlite::SqliteRow) -> MemcoreResult<MemoryEvent> {
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
pub struct SqliteMemoryEventStore {
    pool: SqlitePool,
}

impl SqliteMemoryEventStore {
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
}

#[async_trait]
impl MemoryEventStore for SqliteMemoryEventStore {
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
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(event.id.to_string())
        .bind(&event.org_id)
        .bind(&event.user_id)
        .bind(optional_uuid_to_str(event.fact_id))
        .bind(memory_event_operation_to_str(event.operation))
        .bind(&event.input_text)
        .bind(&event.previous_content)
        .bind(&event.new_content)
        .bind(&event.provider_name)
        .bind(&event.model_name)
        .bind(metadata_to_str(&event.metadata)?)
        .bind(datetime_to_str(event.created_at))
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("failed to insert memory event", error))?;

        Ok(event)
    }

    async fn list_events(&self, query: MemoryEventQuery) -> MemcoreResult<Vec<MemoryEvent>> {
        let limit = normalize_event_list_limit(query.limit)?;
        let _ = query.cursor;

        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, org_id, user_id, fact_id, operation, input_text, previous_content, new_content, provider_name, model_name, metadata, created_at FROM memory_events WHERE org_id = ",
        );
        builder.push_bind(query.tenant.org_id.clone());
        builder.push(" AND user_id = ");
        builder.push_bind(query.tenant.user_id.clone());

        if let Some(fact_id) = query.fact_id {
            builder.push(" AND fact_id = ");
            builder.push_bind(fact_id.to_string());
        }

        if let Some(operation) = query.operation {
            builder.push(" AND operation = ");
            builder.push_bind(memory_event_operation_to_str(operation));
        }

        builder.push(" ORDER BY created_at DESC LIMIT ");
        builder.push_bind(i64::try_from(limit).map_err(|error| {
            storage_error("event list limit out of range for sqlite", error)
        })?);

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| storage_error("failed to list memory events", error))?;

        rows.iter().map(parse_event_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use memcore_common::MemcoreError;
    use memcore_core::{MemoryEvent, MemoryEventOperation, TenantContext};
    use serde_json::json;
    use uuid::Uuid;

    use super::SqliteMemoryEventStore;
    use crate::traits::MemoryEventStore;
    use memcore_core::ports::MemoryEventQuery;

    async fn test_store() -> SqliteMemoryEventStore {
        SqliteMemoryEventStore::connect("sqlite::memory:?cache=shared")
            .await
            .expect("sqlite event store should connect")
    }

    fn tenant(org_id: &str, user_id: &str) -> TenantContext {
        TenantContext::new(org_id, user_id).expect("tenant should be valid")
    }

    fn sample_event(
        org_id: &str,
        user_id: &str,
        fact_id: Option<Uuid>,
        operation: MemoryEventOperation,
        metadata: serde_json::Value,
    ) -> MemoryEvent {
        MemoryEvent::new(
            org_id,
            user_id,
            fact_id,
            operation,
            Some("previous".to_string()),
            Some("new".to_string()),
            Some("mock".to_string()),
            Some("mock-llm".to_string()),
            metadata,
        )
    }

    #[tokio::test]
    async fn record_event_stores_event() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let fact_id = Uuid::new_v4();
        let event = sample_event(
            "org_a",
            "user_a",
            Some(fact_id),
            MemoryEventOperation::Add,
            json!({ "source": "test" }),
        );

        store
            .record_event(&tenant, event.clone())
            .await
            .expect("record should succeed");

        let listed = store
            .list_events(MemoryEventQuery::new(tenant, 10))
            .await
            .expect("list should succeed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, event.id);
        assert_eq!(listed[0].fact_id, Some(fact_id));
    }

    #[tokio::test]
    async fn list_events_returns_tenant_events() {
        let store = test_store().await;
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_b", "user_b");

        store
            .record_event(
                &tenant_a,
                sample_event(
                    "org_a",
                    "user_a",
                    None,
                    MemoryEventOperation::Add,
                    json!({}),
                ),
            )
            .await
            .expect("record should succeed");
        store
            .record_event(
                &tenant_b,
                sample_event(
                    "org_b",
                    "user_b",
                    None,
                    MemoryEventOperation::Add,
                    json!({}),
                ),
            )
            .await
            .expect("record should succeed");

        let listed_a = store
            .list_events(MemoryEventQuery::new(tenant_a, 10))
            .await
            .expect("list should succeed");
        assert_eq!(listed_a.len(), 1);
        assert_eq!(listed_a[0].org_id, "org_a");

        let listed_b = store
            .list_events(MemoryEventQuery::new(tenant_b, 10))
            .await
            .expect("list should succeed");
        assert_eq!(listed_b.len(), 1);
        assert_eq!(listed_b[0].org_id, "org_b");
    }

    #[tokio::test]
    async fn tenant_isolation_prevents_cross_tenant_list() {
        let store = test_store().await;
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");

        store
            .record_event(
                &tenant_a,
                sample_event(
                    "org_a",
                    "user_a",
                    None,
                    MemoryEventOperation::Add,
                    json!({}),
                ),
            )
            .await
            .expect("record should succeed");

        let listed_b = store
            .list_events(MemoryEventQuery::new(tenant_b, 10))
            .await
            .expect("list should succeed");
        assert!(listed_b.is_empty());
    }

    #[tokio::test]
    async fn fact_id_filter_works() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let fact_a = Uuid::new_v4();
        let fact_b = Uuid::new_v4();

        store
            .record_event(
                &tenant,
                sample_event(
                    "org_a",
                    "user_a",
                    Some(fact_a),
                    MemoryEventOperation::Add,
                    json!({}),
                ),
            )
            .await
            .expect("record should succeed");
        store
            .record_event(
                &tenant,
                sample_event(
                    "org_a",
                    "user_a",
                    Some(fact_b),
                    MemoryEventOperation::Update,
                    json!({}),
                ),
            )
            .await
            .expect("record should succeed");

        let mut query = MemoryEventQuery::new(tenant, 10);
        query.fact_id = Some(fact_a);
        let listed = store.list_events(query).await.expect("list should succeed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].fact_id, Some(fact_a));
    }

    #[tokio::test]
    async fn operation_filter_works() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");

        store
            .record_event(
                &tenant,
                sample_event(
                    "org_a",
                    "user_a",
                    None,
                    MemoryEventOperation::Add,
                    json!({}),
                ),
            )
            .await
            .expect("record should succeed");
        store
            .record_event(
                &tenant,
                sample_event(
                    "org_a",
                    "user_a",
                    None,
                    MemoryEventOperation::Delete,
                    json!({}),
                ),
            )
            .await
            .expect("record should succeed");

        let mut query = MemoryEventQuery::new(tenant, 10);
        query.operation = Some(MemoryEventOperation::Delete);
        let listed = store.list_events(query).await.expect("list should succeed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].operation, MemoryEventOperation::Delete);
    }

    #[tokio::test]
    async fn limit_is_respected() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");

        for index in 0..5 {
            store
                .record_event(
                    &tenant,
                    sample_event(
                        "org_a",
                        "user_a",
                        None,
                        MemoryEventOperation::Add,
                        json!({ "index": index }),
                    ),
                )
                .await
                .expect("record should succeed");
        }

        let listed = store
            .list_events(MemoryEventQuery::new(tenant, 2))
            .await
            .expect("list should succeed");
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn metadata_round_trip_works() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let metadata = json!({ "nested": { "flag": true }, "count": 3 });

        store
            .record_event(
                &tenant,
                sample_event(
                    "org_a",
                    "user_a",
                    None,
                    MemoryEventOperation::Add,
                    metadata.clone(),
                ),
            )
            .await
            .expect("record should succeed");

        let listed = store
            .list_events(MemoryEventQuery::new(tenant, 10))
            .await
            .expect("list should succeed");
        assert_eq!(listed[0].metadata, metadata);
    }

    #[tokio::test]
    async fn created_at_round_trip_works() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let created_at = Utc.with_ymd_and_hms(2026, 3, 15, 12, 30, 0).unwrap();
        let mut event = sample_event(
            "org_a",
            "user_a",
            None,
            MemoryEventOperation::NoOp,
            json!({}),
        );
        event.created_at = created_at;

        store
            .record_event(&tenant, event)
            .await
            .expect("record should succeed");

        let listed = store
            .list_events(MemoryEventQuery::new(tenant, 10))
            .await
            .expect("list should succeed");
        assert_eq!(listed[0].created_at, created_at);
    }

    #[tokio::test]
    async fn optional_fields_null_works() {
        let store = test_store().await;
        let tenant = tenant("org_a", "user_a");
        let event = MemoryEvent::new(
            "org_a",
            "user_a",
            None,
            MemoryEventOperation::ForgetUser,
            None,
            None,
            None,
            None,
            json!({ "deleted": true }),
        );

        store
            .record_event(&tenant, event)
            .await
            .expect("record should succeed");

        let listed = store
            .list_events(MemoryEventQuery::new(tenant, 10))
            .await
            .expect("list should succeed");
        assert!(listed[0].fact_id.is_none());
        assert!(listed[0].previous_content.is_none());
        assert!(listed[0].new_content.is_none());
        assert!(listed[0].input_text.is_none());
    }

    #[tokio::test]
    async fn mismatched_tenant_is_rejected_on_record() {
        let store = test_store().await;
        let tenant_b = tenant("org_a", "user_b");
        let event = sample_event(
            "org_a",
            "user_a",
            None,
            MemoryEventOperation::Add,
            json!({}),
        );

        let error = store
            .record_event(&tenant_b, event)
            .await
            .expect_err("cross-tenant record should fail");
        assert_eq!(error, MemcoreError::Forbidden);
    }
}
