mod types;

use std::sync::Arc;

use chrono::Utc;
use memcore_common::{MemcoreError, MemcoreResult};
use uuid::Uuid;

use crate::{
    assemble_context, BuildContextInput, BuildContextOutput, CandidateFact, Fact,
    MemorySearchResult, MemorySource, TenantContext,
};
use crate::privacy::redact_messages_for_extraction;
use crate::ports::MemoryMessage;
use crate::ports::{
    EmbeddingProvider, FactExtractionInput, FactSearchQuery, FactStore, LlmProvider, VectorRecord,
    VectorSearchQuery, VectorStore,
};

pub use types::{
    AddMemoryInput, AddMemoryOutput, DeleteMemoryInput, DeleteMemoryOutput, ForgetUserInput,
    ForgetUserOutput, ListMemoriesInput, ListMemoriesOutput, MemoryOperationSummary,
    SearchMemoryInput, SearchMemoryOutput, DEFAULT_LIST_MEMORIES_LIMIT, DEFAULT_MIN_IMPORTANCE,
    DEFAULT_SEARCH_LIMIT, MAX_LIST_MEMORIES_LIMIT, MAX_SEARCH_LIMIT,
};

pub struct MemoryEngine {
    fact_store: Arc<dyn FactStore>,
    vector_store: Arc<dyn VectorStore>,
    llm_provider: Arc<dyn LlmProvider>,
    embedding_provider: Arc<dyn EmbeddingProvider>,
    min_importance: f32,
    enable_pii_redaction: bool,
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
            enable_pii_redaction: false,
        }
    }

    pub fn with_min_importance(mut self, min_importance: f32) -> Self {
        self.min_importance = min_importance;
        self
    }

    pub fn with_pii_redaction(mut self, enabled: bool) -> Self {
        self.enable_pii_redaction = enabled;
        self
    }

    pub fn pii_redaction_enabled(&self) -> bool {
        self.enable_pii_redaction
    }

    pub async fn add_memory(&self, input: AddMemoryInput) -> MemcoreResult<AddMemoryOutput> {
        validate_tenant(&input.tenant)?;
        validate_messages(&input.messages)?;

        let messages_for_extraction =
            messages_for_llm_extraction(&input.messages, self.enable_pii_redaction);

        let candidates = self
            .llm_provider
            .extract_facts(FactExtractionInput {
                tenant: input.tenant.clone(),
                messages: messages_for_extraction,
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

    pub async fn search_memory(
        &self,
        input: SearchMemoryInput,
    ) -> MemcoreResult<SearchMemoryOutput> {
        validate_tenant(&input.tenant)?;
        validate_query(&input.query)?;
        let limit = normalize_search_limit(input.limit)?;

        let embedding = self.embedding_provider.embed_text(&input.query).await?;

        let vector_results = self
            .vector_store
            .search_vectors(VectorSearchQuery {
                tenant: input.tenant.clone(),
                embedding,
                limit,
                memory_types: input.memory_types,
                metadata_filter: input.metadata_filter,
            })
            .await?;

        let mut results = Vec::with_capacity(vector_results.len());
        for vector_result in vector_results {
            let mut search_result = MemorySearchResult {
                fact_id: vector_result.fact_id,
                content: vector_result.content,
                memory_type: vector_result.memory_type,
                score: vector_result.score,
                confidence: 0.0,
                importance: 0.0,
                valid_at: None,
                metadata: vector_result.metadata,
            };

            if let Some(fact) = self
                .fact_store
                .get_fact(&input.tenant, search_result.fact_id)
                .await?
            {
                search_result.confidence = fact.confidence;
                search_result.importance = fact.importance;
                search_result.valid_at = fact.valid_at;
            }

            results.push(search_result);
        }

        Ok(SearchMemoryOutput { results })
    }

    pub async fn build_context(
        &self,
        input: BuildContextInput,
    ) -> MemcoreResult<BuildContextOutput> {
        validate_tenant(&input.tenant)?;
        validate_query(&input.query)?;
        let max_memories = normalize_context_max_memories(input.max_memories)?;

        let search_output = self
            .search_memory(SearchMemoryInput {
                tenant: input.tenant,
                query: input.query,
                limit: max_memories,
                memory_types: input.memory_types,
                metadata_filter: None,
            })
            .await?;

        let context = assemble_context(&search_output.results, input.include_metadata);

        Ok(BuildContextOutput {
            context,
            memories: search_output.results,
        })
    }

    pub async fn list_memories(
        &self,
        input: ListMemoriesInput,
    ) -> MemcoreResult<ListMemoriesOutput> {
        validate_tenant(&input.tenant)?;
        let limit = normalize_list_limit(input.limit)?;

        let memory_types = input
            .memory_type
            .map(|memory_type| vec![memory_type]);

        let memories = self
            .fact_store
            .search_facts(FactSearchQuery {
                tenant: input.tenant,
                memory_types,
                query_text: None,
                limit,
                cursor: input.cursor,
                include_deleted: input.include_deleted,
            })
            .await?;

        Ok(ListMemoriesOutput {
            memories,
            next_cursor: None,
        })
    }

    pub async fn delete_memory(
        &self,
        input: DeleteMemoryInput,
    ) -> MemcoreResult<DeleteMemoryOutput> {
        validate_tenant(&input.tenant)?;

        let exists = self
            .fact_store
            .get_fact(&input.tenant, input.memory_id)
            .await?;

        if exists.is_none() {
            return Err(MemcoreError::NotFound("memory not found".to_string()));
        }

        self.fact_store
            .soft_delete_fact(&input.tenant, input.memory_id)
            .await?;

        self.vector_store
            .delete_by_fact_id(&input.tenant, input.memory_id)
            .await?;

        Ok(DeleteMemoryOutput { deleted: true })
    }

    pub async fn forget_user(&self, input: ForgetUserInput) -> MemcoreResult<ForgetUserOutput> {
        validate_tenant(&input.tenant)?;

        self.fact_store
            .delete_user_data(&input.tenant)
            .await?;

        self.vector_store
            .delete_by_user(&input.tenant)
            .await?;

        Ok(ForgetUserOutput { deleted: true })
    }
}

fn messages_for_llm_extraction(
    messages: &[MemoryMessage],
    enable_pii_redaction: bool,
) -> Vec<MemoryMessage> {
    if enable_pii_redaction {
        redact_messages_for_extraction(messages)
    } else {
        messages.to_vec()
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

fn validate_query(query: &str) -> MemcoreResult<()> {
    if query.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "query cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn normalize_context_max_memories(max_memories: usize) -> MemcoreResult<usize> {
    if max_memories == 0 {
        return Err(MemcoreError::ValidationError(
            "max_memories must be greater than 0".to_string(),
        ));
    }

    if max_memories > crate::MAX_CONTEXT_MAX_MEMORIES {
        return Err(MemcoreError::ValidationError(format!(
            "max_memories cannot exceed {}",
            crate::MAX_CONTEXT_MAX_MEMORIES
        )));
    }

    Ok(max_memories)
}

fn normalize_list_limit(limit: usize) -> MemcoreResult<usize> {
    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > types::MAX_LIST_MEMORIES_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {}",
            types::MAX_LIST_MEMORIES_LIMIT
        )));
    }

    Ok(limit)
}

fn normalize_search_limit(limit: usize) -> MemcoreResult<usize> {
    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > types::MAX_SEARCH_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {}",
            types::MAX_SEARCH_LIMIT
        )));
    }

    Ok(limit)
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

