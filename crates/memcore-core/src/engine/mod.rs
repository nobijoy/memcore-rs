mod types;

use std::sync::Arc;

use chrono::Utc;
use memcore_common::{MemcoreError, MemcoreResult};
use uuid::Uuid;

use crate::{
    CandidateFact, Fact, MemorySource, TenantContext,
};
use crate::ports::{
    EmbeddingProvider, FactExtractionInput, FactStore, LlmProvider, VectorRecord, VectorStore,
};

pub use types::{
    AddMemoryInput, AddMemoryOutput, MemoryOperationSummary, DEFAULT_MIN_IMPORTANCE,
};

pub struct MemoryEngine {
    fact_store: Arc<dyn FactStore>,
    vector_store: Arc<dyn VectorStore>,
    llm_provider: Arc<dyn LlmProvider>,
    embedding_provider: Arc<dyn EmbeddingProvider>,
    min_importance: f32,
}

impl MemoryEngine {
    pub fn new(
        fact_store: Arc<dyn FactStore>,
        vector_store: Arc<dyn VectorStore>,
        llm_provider: Arc<dyn LlmProvider>,
        embedding_provider: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self {
            fact_store,
            vector_store,
            llm_provider,
            embedding_provider,
            min_importance: types::DEFAULT_MIN_IMPORTANCE,
        }
    }

    pub fn with_min_importance(mut self, min_importance: f32) -> Self {
        self.min_importance = min_importance;
        self
    }

    pub async fn add_memory(&self, input: AddMemoryInput) -> MemcoreResult<AddMemoryOutput> {
        validate_tenant(&input.tenant)?;
        validate_messages(&input.messages)?;

        let candidates = self
            .llm_provider
            .extract_facts(FactExtractionInput {
                tenant: input.tenant.clone(),
                messages: input.messages,
                metadata: input.metadata.clone(),
            })
            .await?;

        let mut summary = MemoryOperationSummary {
            added: 0,
            updated: 0,
            deleted: 0,
            noop: 0,
        };
        let mut memories = Vec::new();

        for candidate in candidates {
            if !passes_importance_threshold(&candidate, self.min_importance) {
                summary.noop += 1;
                continue;
            }

            validate_candidate(&candidate)?;

            let fact = candidate_to_fact(&input.tenant, &candidate, &input.metadata)?;
            let inserted = self
                .fact_store
                .insert_fact(&input.tenant, fact)
                .await?;

            let embedding = self
                .embedding_provider
                .embed_text(&inserted.content)
                .await?;

            let vector_record = VectorRecord {
                id: Uuid::new_v4(),
                fact_id: inserted.id,
                org_id: input.tenant.org_id.clone(),
                user_id: input.tenant.user_id.clone(),
                embedding,
                content: inserted.content.clone(),
                memory_type: inserted.memory_type,
                metadata: inserted.metadata.clone(),
            };

            self.vector_store
                .upsert_vector(&input.tenant, vector_record)
                .await?;

            summary.added += 1;
            memories.push(inserted);
        }

        Ok(AddMemoryOutput {
            added: summary.added,
            updated: summary.updated,
            deleted: summary.deleted,
            noop: summary.noop,
            memories,
        })
    }
}

fn validate_tenant(tenant: &TenantContext) -> MemcoreResult<()> {
    if tenant.org_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "org_id cannot be empty".to_string(),
        ));
    }
    if tenant.user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn validate_messages(messages: &[crate::ports::MemoryMessage]) -> MemcoreResult<()> {
    if messages.is_empty() {
        return Err(MemcoreError::ValidationError(
            "messages cannot be empty".to_string(),
        ));
    }

    for message in messages {
        if message.content.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "message content cannot be empty".to_string(),
            ));
        }
    }

    Ok(())
}

fn validate_candidate(candidate: &CandidateFact) -> MemcoreResult<()> {
    if candidate.content.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "candidate fact content cannot be empty".to_string(),
        ));
    }

    if !(0.0..=1.0).contains(&candidate.confidence) {
        return Err(MemcoreError::ValidationError(
            "candidate confidence must be between 0.0 and 1.0".to_string(),
        ));
    }

    if !(0.0..=1.0).contains(&candidate.importance) {
        return Err(MemcoreError::ValidationError(
            "candidate importance must be between 0.0 and 1.0".to_string(),
        ));
    }

    Ok(())
}

fn passes_importance_threshold(candidate: &CandidateFact, min_importance: f32) -> bool {
    candidate.importance >= min_importance
}

/// Converts a candidate fact into a persisted fact.
///
/// Candidate metadata is preserved as-is for this phase. Input metadata is not merged yet.
fn candidate_to_fact(
    tenant: &TenantContext,
    candidate: &CandidateFact,
    _input_metadata: &serde_json::Value,
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

