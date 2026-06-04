#![cfg(feature = "lancedb")]

use std::sync::Arc;

use arrow_array::{
    Array, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_array::types::Float32Type;
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::Table;
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{MemoryType, TenantContext};
use memcore_core::ports::{VectorRecord, VectorSearchQuery, VectorSearchResult, VectorStore};
use serde_json::Value;
use uuid::Uuid;


fn memory_type_to_str(value: MemoryType) -> &'static str {
    match value {
        MemoryType::Profile => "profile",
        MemoryType::Preference => "preference",
        MemoryType::Project => "project",
        MemoryType::Conversation => "conversation",
        MemoryType::Task => "task",
        MemoryType::Entity => "entity",
        MemoryType::Skill => "skill",
        MemoryType::System => "system",
    }
}

fn memory_type_from_str(value: &str) -> MemcoreResult<MemoryType> {
    match value {
        "profile" => Ok(MemoryType::Profile),
        "preference" => Ok(MemoryType::Preference),
        "project" => Ok(MemoryType::Project),
        "conversation" => Ok(MemoryType::Conversation),
        "task" => Ok(MemoryType::Task),
        "entity" => Ok(MemoryType::Entity),
        "skill" => Ok(MemoryType::Skill),
        "system" => Ok(MemoryType::System),
        _ => Err(MemcoreError::StorageError(format!(
            "invalid memory_type value: {value}"
        ))),
    }
}

const EMBEDDING_COLUMN: &str = "embedding";
const COL_ID: &str = "id";
const COL_FACT_ID: &str = "fact_id";
const COL_ORG_ID: &str = "org_id";
const COL_USER_ID: &str = "user_id";
const COL_CONTENT: &str = "content";
const COL_MEMORY_TYPE: &str = "memory_type";
const COL_METADATA: &str = "metadata";

fn storage_error(context: impl Into<String>, error: impl std::fmt::Display) -> MemcoreError {
    MemcoreError::StorageError(format!("{}: {error}", context.into()))
}

fn ensure_record_tenant(record: &VectorRecord, tenant: &TenantContext) -> MemcoreResult<()> {
    if record.org_id == tenant.org_id && record.user_id == tenant.user_id {
        Ok(())
    } else {
        Err(MemcoreError::Forbidden)
    }
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn tenant_filter(tenant: &TenantContext) -> String {
    format!(
        "{} = '{}' AND {} = '{}'",
        COL_ORG_ID,
        escape_sql_literal(&tenant.org_id),
        COL_USER_ID,
        escape_sql_literal(&tenant.user_id)
    )
}

fn memory_types_filter(types: &[MemoryType]) -> String {
    let list = types
        .iter()
        .map(|t| format!("'{}'", escape_sql_literal(memory_type_to_str(*t))))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{COL_MEMORY_TYPE} IN ({list})")
}

fn distance_to_score(distance: f32) -> f32 {
    // LanceDB returns L2 distance (lower is better). Map to a higher-is-better score.
    1.0 / (1.0 + distance.max(0.0))
}

pub struct LanceDbVectorStore {
    table: Arc<Table>,
    dimensions: usize,
}

impl LanceDbVectorStore {
    pub async fn connect(path: &str, table_name: &str, dimensions: usize) -> MemcoreResult<Self> {
        Self::new_or_open(path, table_name, dimensions).await
    }

    pub async fn new_or_open(
        path: &str,
        table_name: &str,
        dimensions: usize,
    ) -> MemcoreResult<Self> {
        if dimensions == 0 {
            return Err(MemcoreError::ValidationError(
                "embedding dimensions must be greater than 0".to_string(),
            ));
        }

        let db = lancedb::connect(path)
            .execute()
            .await
            .map_err(|e| storage_error("failed to connect lancedb", e))?;

        let table = match db.open_table(table_name).execute().await {
            Ok(table) => table,
            Err(_) => create_table(&db, table_name, dimensions).await?,
        };

        Ok(Self {
            table: Arc::new(table),
            dimensions,
        })
    }

    fn schema(dimensions: usize) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new(COL_ID, DataType::Utf8, false),
            Field::new(COL_FACT_ID, DataType::Utf8, false),
            Field::new(COL_ORG_ID, DataType::Utf8, false),
            Field::new(COL_USER_ID, DataType::Utf8, false),
            Field::new(COL_CONTENT, DataType::Utf8, false),
            Field::new(COL_MEMORY_TYPE, DataType::Utf8, false),
            Field::new(
                EMBEDDING_COLUMN,
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    dimensions as i32,
                ),
                false,
            ),
            Field::new(COL_METADATA, DataType::Utf8, false),
        ]))
    }

    fn record_to_batch(record: &VectorRecord, dimensions: usize) -> MemcoreResult<RecordBatch> {
        if record.embedding.len() != dimensions {
            return Err(MemcoreError::ValidationError(format!(
                "embedding length {} does not match configured dimensions {dimensions}",
                record.embedding.len()
            )));
        }

        let schema = Self::schema(dimensions);
        let embedding_values: Vec<Option<f32>> = record
            .embedding
            .iter()
            .map(|v| Some(*v))
            .collect();
        let embedding_list = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            std::iter::once(Some(embedding_values)),
            dimensions as i32,
        );

        RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec![record.id.to_string()])),
                Arc::new(StringArray::from(vec![record.fact_id.to_string()])),
                Arc::new(StringArray::from(vec![record.org_id.clone()])),
                Arc::new(StringArray::from(vec![record.user_id.clone()])),
                Arc::new(StringArray::from(vec![record.content.clone()])),
                Arc::new(StringArray::from(vec![memory_type_to_str(record.memory_type).to_string()])),
                Arc::new(embedding_list),
                Arc::new(StringArray::from(vec![record.metadata.to_string()])),
            ],
        )
        .map_err(|e| storage_error("failed to build vector record batch", e))
    }

    async fn delete_matching(&self, predicate: &str) -> MemcoreResult<()> {
        self.table
            .delete(predicate)
            .await
            .map_err(|e| storage_error("failed to delete lancedb vectors", e))?;
        Ok(())
    }
}

async fn create_table(
    db: &lancedb::Connection,
    table_name: &str,
    dimensions: usize,
) -> MemcoreResult<Table> {
    let schema = LanceDbVectorStore::schema(dimensions);
    let embedding_values: Vec<Option<f32>> = vec![Some(0.0); dimensions];
    let embedding_list = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        std::iter::once(Some(embedding_values)),
        dimensions as i32,
    );

    let placeholder_id = Uuid::nil().to_string();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(vec![placeholder_id.clone()])),
            Arc::new(StringArray::from(vec![placeholder_id])),
            Arc::new(StringArray::from(vec!["__init__".to_string()])),
            Arc::new(StringArray::from(vec!["__init__".to_string()])),
            Arc::new(StringArray::from(vec!["".to_string()])),
            Arc::new(StringArray::from(vec!["system".to_string()])),
            Arc::new(embedding_list),
            Arc::new(StringArray::from(vec!["{}".to_string()])),
        ],
    )
    .map_err(|e| storage_error("failed to build placeholder batch", e))?;

    let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
    let table = db
        .create_table(table_name, Box::new(batches))
        .execute()
        .await
        .map_err(|e| storage_error("failed to create lancedb table", e))?;

    table
        .delete(&format!("{COL_ORG_ID} = '__init__'"))
        .await
        .map_err(|e| storage_error("failed to remove lancedb placeholder row", e))?;

    Ok(table)
}

fn parse_search_batch(batch: &RecordBatch) -> MemcoreResult<Vec<VectorSearchResult>> {
    let fact_ids = batch
        .column_by_name(COL_FACT_ID)
        .ok_or_else(|| storage_error("missing fact_id column", "column not found"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| storage_error("invalid fact_id column type", "expected utf8"))?;

    let contents = batch
        .column_by_name(COL_CONTENT)
        .ok_or_else(|| storage_error("missing content column", "column not found"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| storage_error("invalid content column type", "expected utf8"))?;

    let memory_types = batch
        .column_by_name(COL_MEMORY_TYPE)
        .ok_or_else(|| storage_error("missing memory_type column", "column not found"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| storage_error("invalid memory_type column type", "expected utf8"))?;

    let metadata_col = batch
        .column_by_name(COL_METADATA)
        .ok_or_else(|| storage_error("missing metadata column", "column not found"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| storage_error("invalid metadata column type", "expected utf8"))?;

    let distances = batch
        .column_by_name("_distance")
        .and_then(|col| col.as_any().downcast_ref::<Float32Array>());

    let mut results = Vec::with_capacity(fact_ids.len());
    for row in 0..fact_ids.len() {
        let fact_id = Uuid::parse_str(fact_ids.value(row))
            .map_err(|e| storage_error("invalid fact_id uuid in lancedb row", e))?;
        let memory_type = memory_type_from_str(memory_types.value(row))?;
        let metadata: Value = serde_json::from_str(metadata_col.value(row))
            .map_err(|e| storage_error("invalid metadata json in lancedb row", e))?;
        let score = distances
            .map(|d| distance_to_score(d.value(row)))
            .unwrap_or(0.0);

        results.push(VectorSearchResult {
            fact_id,
            content: contents.value(row).to_string(),
            score,
            memory_type,
            metadata,
        });
    }

    Ok(results)
}

#[async_trait]
impl VectorStore for LanceDbVectorStore {
    async fn upsert_vector(
        &self,
        tenant: &TenantContext,
        record: VectorRecord,
    ) -> MemcoreResult<()> {
        ensure_record_tenant(&record, tenant)?;

        let delete_predicate = format!(
            "{} AND {} = '{}'",
            tenant_filter(tenant),
            COL_FACT_ID,
            escape_sql_literal(&record.fact_id.to_string())
        );
        self.delete_matching(&delete_predicate).await?;

        let batch = Self::record_to_batch(&record, self.dimensions)?;
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.table
            .add(Box::new(batches))
            .execute()
            .await
            .map_err(|e| storage_error("failed to insert lancedb vector", e))?;

        Ok(())
    }

    async fn search_vectors(
        &self,
        query: VectorSearchQuery,
    ) -> MemcoreResult<Vec<VectorSearchResult>> {
        if query.embedding.len() != self.dimensions {
            return Err(MemcoreError::ValidationError(format!(
                "query embedding length {} does not match configured dimensions {}",
                query.embedding.len(),
                self.dimensions
            )));
        }

        let mut filter = tenant_filter(&query.tenant);
        if let Some(types) = &query.memory_types {
            if !types.is_empty() {
                filter.push_str(" AND ");
                filter.push_str(&memory_types_filter(types));
            }
        }

        // metadata_filter intentionally ignored in this phase (see IMPLEMENTATION_STATUS).
        let _ = query.metadata_filter;

        let mut results = Vec::new();
        let stream = self
            .table
            .query()
            .nearest_to(query.embedding.as_slice())
            .map_err(|e| storage_error("invalid lancedb nearest_to query", e))?
            .column(EMBEDDING_COLUMN)
            .only_if(&filter)
            .limit(query.limit)
            .select(Select::Columns(vec![
                COL_FACT_ID.into(),
                COL_CONTENT.into(),
                COL_MEMORY_TYPE.into(),
                COL_METADATA.into(),
            ]))
            .execute()
            .await
            .map_err(|e| storage_error("failed to execute lancedb vector search", e))?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| storage_error("failed to collect lancedb search results", e))?;

        for batch in batches {
            results.extend(parse_search_batch(&batch)?);
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(query.limit);
        Ok(results)
    }

    async fn delete_by_fact_id(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<()> {
        let predicate = format!(
            "{} AND {} = '{}'",
            tenant_filter(tenant),
            COL_FACT_ID,
            escape_sql_literal(&fact_id.to_string())
        );

        let before = self
            .table
            .count_rows(Some(predicate.clone()))
            .await
            .map_err(|e| storage_error("failed to count lancedb rows before delete", e))?;

        if before == 0 {
            return Err(MemcoreError::NotFound(format!(
                "vector record not found for fact: {fact_id}"
            )));
        }

        self.delete_matching(&predicate).await
    }

    async fn delete_by_user(&self, tenant: &TenantContext) -> MemcoreResult<()> {
        self.delete_matching(&tenant_filter(tenant)).await
    }
}
