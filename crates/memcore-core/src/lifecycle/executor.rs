use chrono::Utc;
use memcore_common::{MemcoreError, MemcoreResult};
use serde_json::Value;
use uuid::Uuid;

use crate::ports::{EmbeddingProvider, FactStore, VectorRecord, VectorStore};
use crate::{CandidateFact, Fact, FactOperation, FactOperationDecision, MemorySource, TenantContext};

/// Outcome of applying a lifecycle operation to a candidate fact.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleApplyResult {
    Added(Fact),
    Updated { previous: Fact, updated: Fact },
    Deleted(Fact),
    NoOp,
}

pub struct LifecycleContext<'a> {
    pub fact_store: &'a dyn FactStore,
    pub vector_store: &'a dyn VectorStore,
    pub embedding_provider: &'a dyn EmbeddingProvider,
}

/// Maps provider decisions to executable operations. Archive/Summarize are treated as NoOp.
pub fn normalize_operation(decision: &FactOperationDecision) -> MemcoreResult<FactOperation> {
    match decision.operation {
        FactOperation::Add => Ok(FactOperation::Add),
        FactOperation::Update => {
            require_target_fact_id(decision, "update")?;
            Ok(FactOperation::Update)
        }
        FactOperation::Delete => {
            require_target_fact_id(decision, "delete")?;
            Ok(FactOperation::Delete)
        }
        FactOperation::NoOp => Ok(FactOperation::NoOp),
        FactOperation::Archive | FactOperation::Summarize => Ok(FactOperation::NoOp),
    }
}

fn require_target_fact_id(decision: &FactOperationDecision, operation: &str) -> MemcoreResult<()> {
    if decision.target_fact_id.is_none() {
        return Err(MemcoreError::ValidationError(format!(
            "target_fact_id is required for {operation} operation"
        )));
    }
    Ok(())
}

pub async fn apply_fact_operation(
    ctx: &LifecycleContext<'_>,
    tenant: &TenantContext,
    candidate: &CandidateFact,
    decision: &FactOperationDecision,
    input_metadata: &Value,
) -> MemcoreResult<LifecycleApplyResult> {
    let operation = normalize_operation(decision)?;

    match operation {
        FactOperation::Add => apply_add(ctx, tenant, candidate, input_metadata).await,
        FactOperation::Update => {
            let target_id = decision
                .target_fact_id
                .expect("target_fact_id validated above");
            apply_update(ctx, tenant, candidate, target_id).await
        }
        FactOperation::Delete => {
            let target_id = decision
                .target_fact_id
                .expect("target_fact_id validated above");
            apply_delete(ctx, tenant, target_id).await
        }
        FactOperation::NoOp => Ok(LifecycleApplyResult::NoOp),
        FactOperation::Archive | FactOperation::Summarize => Ok(LifecycleApplyResult::NoOp),
    }
}

async fn apply_add(
    ctx: &LifecycleContext<'_>,
    tenant: &TenantContext,
    candidate: &CandidateFact,
    input_metadata: &Value,
) -> MemcoreResult<LifecycleApplyResult> {
    let fact = candidate_to_fact(tenant, candidate, input_metadata)?;
    let inserted = ctx.fact_store.insert_fact(tenant, fact).await?;
    upsert_fact_vector(ctx, tenant, &inserted).await?;
    Ok(LifecycleApplyResult::Added(inserted))
}

async fn apply_update(
    ctx: &LifecycleContext<'_>,
    tenant: &TenantContext,
    candidate: &CandidateFact,
    target_fact_id: Uuid,
) -> MemcoreResult<LifecycleApplyResult> {
    let existing = ctx
        .fact_store
        .get_fact(tenant, target_fact_id)
        .await?
        .ok_or_else(|| MemcoreError::NotFound("memory not found".to_string()))?;

    let updated = merge_candidate_into_fact(&existing, candidate)?;
    let stored = ctx.fact_store.update_fact(tenant, updated).await?;
    replace_fact_vector(ctx, tenant, &stored).await?;
    Ok(LifecycleApplyResult::Updated {
        previous: existing,
        updated: stored,
    })
}

async fn apply_delete(
    ctx: &LifecycleContext<'_>,
    tenant: &TenantContext,
    target_fact_id: Uuid,
) -> MemcoreResult<LifecycleApplyResult> {
    let existing = ctx
        .fact_store
        .get_fact(tenant, target_fact_id)
        .await?
        .ok_or_else(|| MemcoreError::NotFound("memory not found".to_string()))?;

    ctx.fact_store
        .soft_delete_fact(tenant, target_fact_id)
        .await?;

    match ctx
        .vector_store
        .delete_by_fact_id(tenant, target_fact_id)
        .await
    {
        Ok(()) => {}
        Err(MemcoreError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }

    Ok(LifecycleApplyResult::Deleted(existing))
}

async fn upsert_fact_vector(
    ctx: &LifecycleContext<'_>,
    tenant: &TenantContext,
    fact: &Fact,
) -> MemcoreResult<()> {
    let embedding = ctx.embedding_provider.embed_text(&fact.content).await?;
    let record = vector_record_from_fact(fact, embedding);
    ctx.vector_store.upsert_vector(tenant, record).await
}

async fn replace_fact_vector(
    ctx: &LifecycleContext<'_>,
    tenant: &TenantContext,
    fact: &Fact,
) -> MemcoreResult<()> {
    match ctx.vector_store.delete_by_fact_id(tenant, fact.id).await {
        Ok(()) | Err(MemcoreError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }
    upsert_fact_vector(ctx, tenant, fact).await
}

fn vector_record_from_fact(fact: &Fact, embedding: Vec<f32>) -> VectorRecord {
    VectorRecord {
        id: Uuid::new_v4(),
        fact_id: fact.id,
        org_id: fact.org_id.clone(),
        user_id: fact.user_id.clone(),
        embedding,
        content: fact.content.clone(),
        memory_type: fact.memory_type,
        metadata: fact.metadata.clone(),
    }
}

fn merge_candidate_into_fact(existing: &Fact, candidate: &CandidateFact) -> MemcoreResult<Fact> {
    let now = Utc::now();
    Fact::new(
        existing.id,
        existing.org_id.clone(),
        existing.user_id.clone(),
        candidate.memory_type,
        candidate.content.clone(),
        existing.summary.clone(),
        existing.source,
        candidate.confidence,
        candidate.importance,
        candidate.valid_at,
        existing.invalid_at,
        existing.recorded_at,
        now,
        candidate.metadata.clone(),
    )
}

fn candidate_to_fact(
    tenant: &TenantContext,
    candidate: &CandidateFact,
    _input_metadata: &Value,
) -> MemcoreResult<Fact> {
    let now = Utc::now();
    Fact::new(
        Uuid::new_v4(),
        tenant.org_id.clone(),
        tenant.user_id.clone(),
        candidate.memory_type,
        candidate.content.clone(),
        None,
        MemorySource::UserMessage,
        candidate.confidence,
        candidate.importance,
        candidate.valid_at,
        None,
        now,
        now,
        candidate.metadata.clone(),
    )
}
