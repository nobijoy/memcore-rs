#![cfg(feature = "qdrant")]

use async_trait::async_trait;
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::ports::{VectorRecord, VectorSearchQuery, VectorSearchResult, VectorStore};
use memcore_core::{MemoryType, TenantContext};
use qdrant_client::Payload;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, CountPointsBuilder, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter,
    PointStruct, QueryPointsBuilder, UpsertPointsBuilder, Value as QdrantValue,
    VectorParamsBuilder, value::Kind,
};
use std::collections::HashMap;
use uuid::Uuid;

const PAYLOAD_ID: &str = "id";
const PAYLOAD_FACT_ID: &str = "fact_id";
const PAYLOAD_ORG_ID: &str = "org_id";
const PAYLOAD_USER_ID: &str = "user_id";
const PAYLOAD_CONTENT: &str = "content";
const PAYLOAD_MEMORY_TYPE: &str = "memory_type";
const PAYLOAD_METADATA: &str = "metadata";

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

fn tenant_filter(tenant: &TenantContext) -> Filter {
    Filter::must([
        Condition::matches(PAYLOAD_ORG_ID, tenant.org_id.clone()),
        Condition::matches(PAYLOAD_USER_ID, tenant.user_id.clone()),
    ])
}

fn tenant_fact_filter(tenant: &TenantContext, fact_id: Uuid) -> Filter {
    Filter::must([
        Condition::matches(PAYLOAD_ORG_ID, tenant.org_id.clone()),
        Condition::matches(PAYLOAD_USER_ID, tenant.user_id.clone()),
        Condition::matches(PAYLOAD_FACT_ID, fact_id.to_string()),
    ])
}

fn search_filter(query: &VectorSearchQuery) -> Filter {
    let mut must = vec![
        Condition::matches(PAYLOAD_ORG_ID, query.tenant.org_id.clone()),
        Condition::matches(PAYLOAD_USER_ID, query.tenant.user_id.clone()),
    ];

    if let Some(types) = &query.memory_types {
        if !types.is_empty() {
            let type_conditions = types
                .iter()
                .map(|memory_type| {
                    Condition::matches(
                        PAYLOAD_MEMORY_TYPE,
                        memory_type_to_str(*memory_type).to_string(),
                    )
                })
                .collect::<Vec<_>>();
            must.push(Filter::should(type_conditions).into());
        }
    }

    Filter::must(must)
}

fn record_to_payload(record: &VectorRecord) -> MemcoreResult<Payload> {
    Payload::try_from(serde_json::json!({
        PAYLOAD_ID: record.id.to_string(),
        PAYLOAD_FACT_ID: record.fact_id.to_string(),
        PAYLOAD_ORG_ID: record.org_id,
        PAYLOAD_USER_ID: record.user_id,
        PAYLOAD_CONTENT: record.content,
        PAYLOAD_MEMORY_TYPE: memory_type_to_str(record.memory_type),
        PAYLOAD_METADATA: record.metadata.to_string(),
    }))
    .map_err(|error| storage_error("failed to build qdrant payload", error))
}

fn value_string(value: &QdrantValue) -> MemcoreResult<String> {
    match &value.kind {
        Some(Kind::StringValue(value)) => Ok(value.clone()),
        _ => Err(storage_error(
            "invalid qdrant payload value",
            "expected string value",
        )),
    }
}

fn map_string(payload: &HashMap<String, QdrantValue>, key: &str) -> MemcoreResult<String> {
    let value = payload
        .get(key)
        .ok_or_else(|| storage_error("missing qdrant payload field", format!("{key} missing")))?;
    value_string(value)
}

fn scored_point_to_result(
    point: qdrant_client::qdrant::ScoredPoint,
) -> MemcoreResult<VectorSearchResult> {
    if point.payload.is_empty() {
        return Err(storage_error(
            "missing qdrant point payload",
            "payload absent",
        ));
    }

    let fact_id = Uuid::parse_str(&map_string(&point.payload, PAYLOAD_FACT_ID)?)
        .map_err(|error| storage_error("invalid fact_id uuid in qdrant payload", error))?;
    let memory_type = memory_type_from_str(&map_string(&point.payload, PAYLOAD_MEMORY_TYPE)?)?;
    let metadata: serde_json::Value =
        serde_json::from_str(&map_string(&point.payload, PAYLOAD_METADATA)?)
            .map_err(|error| storage_error("invalid metadata json in qdrant payload", error))?;

    Ok(VectorSearchResult {
        fact_id,
        content: map_string(&point.payload, PAYLOAD_CONTENT)?,
        score: point.score,
        memory_type,
        metadata,
    })
}

pub struct QdrantVectorStore {
    client: Qdrant,
    collection_name: String,
    dimensions: usize,
}

impl QdrantVectorStore {
    pub async fn connect(
        url: &str,
        collection_name: &str,
        dimensions: usize,
    ) -> MemcoreResult<Self> {
        if dimensions == 0 {
            return Err(MemcoreError::ValidationError(
                "embedding dimensions must be greater than 0".to_string(),
            ));
        }

        let client = Qdrant::from_url(url)
            .build()
            .map_err(|error| storage_error("failed to connect qdrant", error))?;

        ensure_collection(&client, collection_name, dimensions).await?;

        Ok(Self {
            client,
            collection_name: collection_name.to_string(),
            dimensions,
        })
    }

    async fn delete_matching(&self, filter: Filter) -> MemcoreResult<()> {
        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection_name)
                    .points(filter)
                    .wait(true),
            )
            .await
            .map_err(|error| storage_error("failed to delete qdrant points", error))?;
        Ok(())
    }

    async fn count_matching(&self, filter: Filter) -> MemcoreResult<u64> {
        let response = self
            .client
            .count(
                CountPointsBuilder::new(&self.collection_name)
                    .filter(filter)
                    .exact(true),
            )
            .await
            .map_err(|error| storage_error("failed to count qdrant points", error))?;
        Ok(response.result.map(|count| count.count).unwrap_or(0))
    }
}

async fn ensure_collection(
    client: &Qdrant,
    collection_name: &str,
    dimensions: usize,
) -> MemcoreResult<()> {
    let exists = client
        .collection_exists(collection_name)
        .await
        .map_err(|error| storage_error("failed to check qdrant collection", error))?;

    if exists {
        return Ok(());
    }

    client
        .create_collection(
            CreateCollectionBuilder::new(collection_name).vectors_config(VectorParamsBuilder::new(
                dimensions as u64,
                Distance::Cosine,
            )),
        )
        .await
        .map_err(|error| storage_error("failed to create qdrant collection", error))?;

    Ok(())
}

#[async_trait]
impl VectorStore for QdrantVectorStore {
    async fn upsert_vector(
        &self,
        tenant: &TenantContext,
        record: VectorRecord,
    ) -> MemcoreResult<()> {
        ensure_record_tenant(&record, tenant)?;

        if record.embedding.len() != self.dimensions {
            return Err(MemcoreError::ValidationError(format!(
                "embedding length {} does not match configured dimensions {}",
                record.embedding.len(),
                self.dimensions
            )));
        }

        self.delete_matching(tenant_fact_filter(tenant, record.fact_id))
            .await?;

        let payload = record_to_payload(&record)?;
        let point = PointStruct::new(record.id.to_string(), record.embedding, payload);

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, vec![point]).wait(true))
            .await
            .map_err(|error| storage_error("failed to upsert qdrant vector", error))?;

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

        // metadata_filter intentionally ignored in this phase.
        let filter = search_filter(&query);
        let embedding = query.embedding;

        let response = self
            .client
            .query(
                QueryPointsBuilder::new(&self.collection_name)
                    .query(embedding)
                    .filter(filter)
                    .limit(query.limit as u64)
                    .with_payload(true),
            )
            .await
            .map_err(|error| storage_error("failed to query qdrant vectors", error))?;

        let mut results = Vec::with_capacity(response.result.len());
        for point in response.result {
            results.push(scored_point_to_result(point)?);
        }

        results.truncate(query.limit);
        Ok(results)
    }

    async fn delete_by_fact_id(&self, tenant: &TenantContext, fact_id: Uuid) -> MemcoreResult<()> {
        let filter = tenant_fact_filter(tenant, fact_id);
        if self.count_matching(filter.clone()).await? == 0 {
            return Err(MemcoreError::NotFound(format!(
                "vector record not found for fact: {fact_id}"
            )));
        }

        self.delete_matching(filter).await
    }

    async fn delete_by_user(&self, tenant: &TenantContext) -> MemcoreResult<()> {
        self.delete_matching(tenant_filter(tenant)).await
    }
}
