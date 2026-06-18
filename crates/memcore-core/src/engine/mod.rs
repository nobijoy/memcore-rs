mod types;

use std::sync::Arc;

use chrono::{Duration, Utc};
use memcore_common::MemcoreResult;

use crate::admin::{
    CreateMemoryUsageSnapshotInput, CreateMemoryUsageSnapshotOutput, DEFAULT_LIST_ORG_USERS_LIMIT,
    ListOrgUsersInput, ListOrgUsersOutput, MAX_LIST_ORG_USERS_LIMIT,
    MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT, MemoryUsageLatestSnapshot, MemoryUsageSnapshot,
    OrgMemoryUsageSummary, OrgSummaryInput, OrgSummaryOutput, OrgUsageDashboardInput,
    OrgUsageDashboardOutput, ProviderUsageDailyInput, ProviderUsageDailyOutput,
    QueryMemoryUsageSnapshotsInput, QueryMemoryUsageSnapshotsOutput, SearchOrgMemoryEventsInput,
    SearchOrgMemoryEventsOutput, empty_provider_usage_summary,
    validate_memory_usage_snapshot_limit,
};
use crate::audit::{
    build_add_event, build_delete_event, build_forget_user_event, build_import_replace_event,
    build_noop_event, build_update_event, record_event_best_effort,
};
use crate::context_cache_metrics_recorder;
use crate::dedup::{
    DeduplicationDecision, EmbeddingDeduplicationConfig, detect_duplicate,
    detect_embedding_duplicate, find_existing_facts_for_dedup,
};
use crate::export::{EXPORT_EVENTS_LIMIT, EXPORT_FACTS_LIMIT, UserMemoryExport};
use crate::import::{
    ImportMode, ImportUserDataInput, ImportUserDataOutput, ImportValidationSummary,
    collect_import_validation, resolve_import_fact_id,
};
use crate::importance::ImportanceScorer;
use crate::lifecycle::{
    LifecycleApplyResult, LifecycleContext, apply_fact_operation, find_related_facts,
};
use crate::pagination::{PageCursor, build_page, parse_optional_cursor};
use crate::ports::{DEFAULT_PROVIDER_USAGE_LIMIT, MemoryMessage};
use crate::ports::{
    EmbeddingProvider, FactClassificationInput, FactExtractionInput, FactSearchQuery, FactStore,
    LlmProvider, MemoryEventQuery, MemoryEventStore, MemoryUsageSnapshotQuery,
    MemoryUsageSnapshotStore, OrgMemoryEventQuery, OrgPlanStore, OrgUserListQuery,
    ProviderUsageAttributionSlot, ProviderUsageDailyQuery, ProviderUsageQuery, ProviderUsageStore,
    VectorRecord, VectorStore, validate_event_date_range,
};
use crate::privacy::redact_messages_for_extraction;
use crate::quota::{
    CheckMemoryWriteQuotaInput, CheckProviderQuotaInput, GetOrgQuotaStatusInput, QuotaCheckResult,
    QuotaService, ResolvedOrgQuotaLimits,
};
use crate::ranking::{
    RankedCandidate, RankingConfig, RankingSource, RrfConfig, reciprocal_rank_fusion,
    weighted_score_for,
};
use crate::retention::{
    ApplyProviderUsageRetentionInput, ApplyProviderUsageRetentionOutput, ApplyRetentionInput,
    ApplyRetentionOutput,
};
use crate::{
    BuildContextInput, BuildContextOutput, ContextCache, ContextCacheConfig,
    ContextCacheMetricsRecorder, ContextCacheMetricsSnapshot, ContextCacheUsage, Fact,
    FactOperation, FactOperationDecision, MemoryEvent, MemorySearchResult, OrgQuotaLimits,
    SimpleTokenEstimator, TenantContext, apply_provider_compression_summary,
    assemble_context_with_budget, build_context_cache_key, cached_entry_from_output,
};
use crate::{ContextCacheCoordinator, DEFAULT_CONTEXT_CACHE_MAX_ENTRIES, InMemoryContextCache};
use uuid::Uuid;

pub use types::{
    AddMemoryInput, AddMemoryOutput, DEFAULT_LIST_MEMORIES_LIMIT, DEFAULT_LIST_MEMORY_EVENTS_LIMIT,
    DEFAULT_MIN_IMPORTANCE, DEFAULT_SEARCH_LIMIT, DeleteMemoryInput, DeleteMemoryOutput,
    ExportUserDataInput, ForgetUserInput, ForgetUserOutput, ListMemoriesInput, ListMemoriesOutput,
    ListMemoryEventsInput, ListMemoryEventsOutput, MAX_LIST_MEMORIES_LIMIT,
    MAX_LIST_MEMORY_EVENTS_LIMIT, MAX_SEARCH_LIMIT, MemoryOperationSummary, SearchMemoryInput,
    SearchMemoryOutput,
};

pub struct MemoryEngine {
    fact_store: Arc<dyn FactStore>,
    vector_store: Arc<dyn VectorStore>,
    llm_provider: Arc<dyn LlmProvider>,
    embedding_provider: Arc<dyn EmbeddingProvider>,
    event_store: Option<Arc<dyn MemoryEventStore>>,
    audit_provider_name: Option<String>,
    audit_model_name: Option<String>,
    min_importance: f32,
    enable_pii_redaction: bool,
    embedding_dedup_config: EmbeddingDeduplicationConfig,
    ranking_config: RankingConfig,
    rrf_config: RrfConfig,
    context_cache: Arc<dyn ContextCache>,
    context_cache_config: ContextCacheConfig,
    context_cache_coordinator: ContextCacheCoordinator,
    context_cache_metrics: Arc<dyn ContextCacheMetricsRecorder>,
    usage_attribution: Option<Arc<ProviderUsageAttributionSlot>>,
    provider_usage_store: Option<Arc<dyn ProviderUsageStore>>,
    org_plan_store: Option<Arc<dyn OrgPlanStore>>,
    memory_usage_snapshot_store: Option<Arc<dyn MemoryUsageSnapshotStore>>,
    global_quota_limits: OrgQuotaLimits,
}

impl MemoryEngine {
    pub fn new(
        fact_store: Arc<dyn FactStore>,
        vector_store: Arc<dyn VectorStore>,
        llm_provider: Arc<dyn LlmProvider>,
        embedding_provider: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        let cache = Arc::new(InMemoryContextCache::new(DEFAULT_CONTEXT_CACHE_MAX_ENTRIES));
        let cache_config = ContextCacheConfig::default();
        let metrics = context_cache_metrics_recorder(&cache_config);
        let coordinator =
            ContextCacheCoordinator::new(cache.clone(), cache_config, metrics.clone());
        Self {
            fact_store,
            vector_store,
            llm_provider,
            embedding_provider,
            event_store: None,
            audit_provider_name: None,
            audit_model_name: None,
            min_importance: types::DEFAULT_MIN_IMPORTANCE,
            enable_pii_redaction: false,
            embedding_dedup_config: EmbeddingDeduplicationConfig::default(),
            ranking_config: RankingConfig::default(),
            rrf_config: RrfConfig::default(),
            context_cache: cache,
            context_cache_config: cache_config,
            context_cache_coordinator: coordinator,
            context_cache_metrics: metrics,
            usage_attribution: None,
            provider_usage_store: None,
            org_plan_store: None,
            memory_usage_snapshot_store: None,
            global_quota_limits: OrgQuotaLimits::disabled(),
        }
    }

    pub fn with_event_store(mut self, event_store: Arc<dyn MemoryEventStore>) -> Self {
        self.event_store = Some(event_store);
        self
    }

    pub fn with_org_plan_store(mut self, org_plan_store: Arc<dyn OrgPlanStore>) -> Self {
        self.org_plan_store = Some(org_plan_store);
        self
    }

    pub fn with_global_quota_limits(mut self, limits: OrgQuotaLimits) -> Self {
        self.global_quota_limits = limits;
        self
    }

    pub fn with_audit_provider_info(
        mut self,
        provider_name: Option<String>,
        model_name: Option<String>,
    ) -> Self {
        self.audit_provider_name = provider_name;
        self.audit_model_name = model_name;
        self
    }

    pub fn with_min_importance(mut self, min_importance: f32) -> Self {
        self.min_importance = min_importance;
        self
    }

    pub fn with_pii_redaction(mut self, enabled: bool) -> Self {
        self.enable_pii_redaction = enabled;
        self
    }

    pub fn with_embedding_dedup_config(mut self, config: EmbeddingDeduplicationConfig) -> Self {
        self.embedding_dedup_config = config;
        self
    }

    pub fn with_ranking_config(mut self, config: RankingConfig) -> Self {
        self.ranking_config = config;
        self
    }

    pub fn with_rrf_config(mut self, config: RrfConfig) -> Self {
        self.rrf_config = config;
        self
    }

    pub fn with_context_cache(
        mut self,
        cache: Arc<dyn ContextCache>,
        config: ContextCacheConfig,
    ) -> Self {
        let metrics = context_cache_metrics_recorder(&config);
        self.context_cache = cache.clone();
        self.context_cache_config = config;
        self.context_cache_metrics = metrics.clone();
        self.context_cache_coordinator = ContextCacheCoordinator::new(cache, config, metrics);
        self
    }

    pub fn context_cache_config(&self) -> ContextCacheConfig {
        self.context_cache_config
    }

    pub fn context_cache_metrics_snapshot(&self) -> ContextCacheMetricsSnapshot {
        self.context_cache_metrics.snapshot()
    }

    pub fn with_usage_attribution(mut self, slot: Arc<ProviderUsageAttributionSlot>) -> Self {
        self.usage_attribution = Some(slot);
        self
    }

    pub fn with_provider_usage_store(mut self, store: Option<Arc<dyn ProviderUsageStore>>) -> Self {
        self.provider_usage_store = store;
        self
    }

    pub fn with_memory_usage_snapshot_store(
        mut self,
        store: Option<Arc<dyn MemoryUsageSnapshotStore>>,
    ) -> Self {
        self.memory_usage_snapshot_store = store;
        self
    }

    fn bind_usage_attribution(&self, tenant: &TenantContext) {
        if let Some(slot) = &self.usage_attribution {
            slot.set(tenant.org_id.clone(), Some(tenant.user_id.clone()));
        }
    }

    pub fn pii_redaction_enabled(&self) -> bool {
        self.enable_pii_redaction
    }

    pub async fn add_memory(&self, input: AddMemoryInput) -> MemcoreResult<AddMemoryOutput> {
        validate_tenant(&input.tenant)?;
        validate_messages(&input.messages)?;
        self.bind_usage_attribution(&input.tenant);

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

        let lifecycle_ctx = LifecycleContext {
            fact_store: self.fact_store.as_ref(),
            vector_store: self.vector_store.as_ref(),
            embedding_provider: self.embedding_provider.as_ref(),
        };

        for candidate in candidates {
            validate_candidate(&candidate)?;

            let candidate = ImportanceScorer::adjust(&candidate);

            if !passes_importance_threshold(&candidate, self.min_importance) {
                summary.noop += 1;
                continue;
            }

            let existing_for_dedup = find_existing_facts_for_dedup(
                self.fact_store.as_ref(),
                &input.tenant,
                candidate.memory_type,
            )
            .await?;

            if let DeduplicationDecision::Duplicate {
                existing_fact_id,
                reason,
            } = detect_duplicate(&candidate, &existing_for_dedup)
            {
                summary.noop += 1;
                record_event_best_effort(
                    &self.event_store,
                    &input.tenant,
                    build_noop_event(
                        &input.tenant,
                        &dedup_noop_decision(existing_fact_id, reason),
                        &self.audit_provider_name,
                        &self.audit_model_name,
                    ),
                )
                .await;
                continue;
            }

            let candidate_embedding = self
                .embedding_provider
                .embed_text(&candidate.content)
                .await?;

            if let Some(DeduplicationDecision::Duplicate {
                existing_fact_id,
                reason,
            }) = detect_embedding_duplicate(
                self.vector_store.as_ref(),
                &input.tenant,
                candidate.memory_type,
                &candidate_embedding,
                &self.embedding_dedup_config,
            )
            .await?
            {
                summary.noop += 1;
                record_event_best_effort(
                    &self.event_store,
                    &input.tenant,
                    build_noop_event(
                        &input.tenant,
                        &dedup_noop_decision(existing_fact_id, reason),
                        &self.audit_provider_name,
                        &self.audit_model_name,
                    ),
                )
                .await;
                continue;
            }

            let related_facts =
                find_related_facts(self.fact_store.as_ref(), &input.tenant, &candidate).await?;

            let decision = self
                .llm_provider
                .classify_fact_operation(FactClassificationInput {
                    tenant: input.tenant.clone(),
                    candidate_fact: candidate.clone(),
                    existing_facts: related_facts,
                })
                .await?;

            let result = apply_fact_operation(
                &lifecycle_ctx,
                &input.tenant,
                &candidate,
                &decision,
                &input.metadata,
                Some(candidate_embedding),
            )
            .await?;

            match result {
                LifecycleApplyResult::Added(fact) => {
                    summary.added += 1;
                    record_event_best_effort(
                        &self.event_store,
                        &input.tenant,
                        build_add_event(
                            &input.tenant,
                            &fact,
                            &self.audit_provider_name,
                            &self.audit_model_name,
                            input.metadata.clone(),
                        ),
                    )
                    .await;
                    memories.push(fact);
                }
                LifecycleApplyResult::Updated { previous, updated } => {
                    summary.updated += 1;
                    record_event_best_effort(
                        &self.event_store,
                        &input.tenant,
                        build_update_event(
                            &input.tenant,
                            &previous,
                            &updated,
                            &self.audit_provider_name,
                            &self.audit_model_name,
                            input.metadata.clone(),
                        ),
                    )
                    .await;
                    memories.push(updated);
                }
                LifecycleApplyResult::Deleted(fact) => {
                    summary.deleted += 1;
                    record_event_best_effort(
                        &self.event_store,
                        &input.tenant,
                        build_delete_event(
                            &input.tenant,
                            &fact,
                            &self.audit_provider_name,
                            &self.audit_model_name,
                            input.metadata.clone(),
                        ),
                    )
                    .await;
                }
                LifecycleApplyResult::NoOp => {
                    summary.noop += 1;
                    record_event_best_effort(
                        &self.event_store,
                        &input.tenant,
                        build_noop_event(
                            &input.tenant,
                            &decision,
                            &self.audit_provider_name,
                            &self.audit_model_name,
                        ),
                    )
                    .await;
                }
            }
        }

        if summary.added + summary.updated + summary.deleted > 0 {
            self.invalidate_user_context_cache(&input.tenant).await;
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
        use std::cmp::Ordering;
        use std::collections::HashMap;

        validate_tenant(&input.tenant)?;
        validate_query(&input.query)?;
        self.bind_usage_attribution(&input.tenant);
        let limit = normalize_search_limit(input.limit)?;
        let internal_limit = internal_search_limit(limit);

        let embedding = self.embedding_provider.embed_text(&input.query).await?;

        let vector_results = self
            .vector_store
            .search_vectors(crate::ports::VectorSearchQuery {
                tenant: input.tenant.clone(),
                embedding,
                limit: internal_limit,
                memory_types: input.memory_types.clone(),
                metadata_filter: input.metadata_filter,
            })
            .await?;

        let keyword_facts = self
            .fact_store
            .search_facts(FactSearchQuery {
                tenant: input.tenant.clone(),
                memory_types: input.memory_types.clone(),
                query_text: Some(input.query.clone()),
                limit: internal_limit,
                cursor: None,
                include_deleted: false,
            })
            .await?;

        let semantic_candidates: Vec<RankedCandidate> = vector_results
            .iter()
            .enumerate()
            .map(|(index, vector_result)| RankedCandidate {
                fact_id: vector_result.fact_id,
                source: RankingSource::Semantic,
                rank: index + 1,
                score: vector_result.score,
            })
            .collect();

        let keyword_candidates: Vec<RankedCandidate> = keyword_facts
            .iter()
            .enumerate()
            .map(|(index, fact)| RankedCandidate {
                fact_id: fact.id,
                source: RankingSource::Keyword,
                rank: index + 1,
                score: 0.0,
            })
            .collect();

        let fused =
            reciprocal_rank_fusion(&semantic_candidates, &keyword_candidates, &self.rrf_config);

        let vector_by_fact: HashMap<Uuid, _> = vector_results
            .iter()
            .map(|result| (result.fact_id, result))
            .collect();
        let keyword_by_fact: HashMap<Uuid, _> =
            keyword_facts.iter().map(|fact| (fact.id, fact)).collect();
        let now = Utc::now();

        let mut scored: Vec<(MemorySearchResult, f32, f32, chrono::DateTime<Utc>)> =
            Vec::with_capacity(fused.len());

        for (fact_id, rrf_score) in fused {
            let fact = if let Some(fact) = keyword_by_fact.get(&fact_id) {
                Some((*fact).clone())
            } else {
                self.fact_store.get_fact(&input.tenant, fact_id).await?
            };

            let Some(fact) = fact else {
                continue;
            };

            let vector_result = vector_by_fact.get(&fact_id);
            let semantic_score = vector_result.map(|result| result.score).unwrap_or(0.0);
            let content = vector_result
                .map(|result| result.content.clone())
                .unwrap_or_else(|| fact.content.clone());
            let metadata = vector_result
                .map(|result| result.metadata.clone())
                .unwrap_or_else(|| fact.metadata.clone());

            let weighted = weighted_score_for(
                semantic_score,
                fact.importance,
                fact.confidence,
                Some(fact.updated_at),
                &fact.memory_type,
                now,
                &self.ranking_config,
            );

            scored.push((
                MemorySearchResult {
                    fact_id: fact.id,
                    content,
                    memory_type: fact.memory_type,
                    score: rrf_score,
                    confidence: fact.confidence,
                    importance: fact.importance,
                    valid_at: fact.valid_at,
                    metadata,
                },
                rrf_score,
                weighted,
                fact.updated_at,
            ));
        }

        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(Ordering::Equal)
                .then(right.2.partial_cmp(&left.2).unwrap_or(Ordering::Equal))
                .then(right.3.cmp(&left.3))
                .then_with(|| left.0.fact_id.cmp(&right.0.fact_id))
        });
        scored.truncate(limit);

        let results = scored
            .into_iter()
            .map(|(mut result, rrf_score, _, _)| {
                result.score = rrf_score;
                result
            })
            .collect();

        Ok(SearchMemoryOutput { results })
    }

    pub async fn build_context(
        &self,
        input: BuildContextInput,
    ) -> MemcoreResult<BuildContextOutput> {
        validate_tenant(&input.tenant)?;
        validate_query(&input.query)?;
        self.bind_usage_attribution(&input.tenant);
        input.budget.validate()?;
        input
            .compression_options
            .validate(input.budget.available_tokens())?;
        self.context_cache_config.validate()?;

        if !self.context_cache_config.enabled {
            return self.build_context_uncached(input).await;
        }

        let cache_key = build_context_cache_key(&input);
        let (entry, cache_usage) = self
            .context_cache_coordinator
            .get_or_compute(cache_key, || async {
                let output = self.build_context_uncached(input).await?;
                Ok(cached_entry_from_output(
                    &output,
                    &self.context_cache_config,
                ))
            })
            .await?;

        Ok(BuildContextOutput {
            context: entry.context,
            memories: entry.memories,
            budget: entry.budget,
            compression: entry.compression,
            cache: cache_usage,
        })
    }

    /// Recomputes and stores a stale context cache entry (background refresh).
    pub async fn refresh_stale_context(&self, input: BuildContextInput) -> MemcoreResult<()> {
        validate_tenant(&input.tenant)?;
        validate_query(&input.query)?;
        self.bind_usage_attribution(&input.tenant);
        input.budget.validate()?;
        input
            .compression_options
            .validate(input.budget.available_tokens())?;
        self.context_cache_config.validate()?;

        if !self.context_cache_config.enabled {
            return Ok(());
        }

        let cache_key = build_context_cache_key(&input);
        self.context_cache_coordinator
            .refresh_entry(cache_key, || async {
                let output = self.build_context_uncached(input).await?;
                Ok(cached_entry_from_output(
                    &output,
                    &self.context_cache_config,
                ))
            })
            .await
    }

    async fn build_context_uncached(
        &self,
        input: BuildContextInput,
    ) -> MemcoreResult<BuildContextOutput> {
        let max_memories = normalize_context_max_memories(input.max_memories)?;
        let tenant = input.tenant.clone();
        let compression_options = input.compression_options.clone();
        let format_options = input.format_options.clone();

        let search_output = self
            .search_memory(SearchMemoryInput {
                tenant: input.tenant,
                query: input.query,
                limit: max_memories,
                memory_types: input.memory_types,
                metadata_filter: None,
            })
            .await?;

        let mut assembled = assemble_context_with_budget(
            &search_output.results,
            input.include_metadata,
            &format_options,
            &input.budget,
            &compression_options,
            &SimpleTokenEstimator,
        );

        if matches!(
            compression_options.mode,
            crate::ContextCompressionMode::ProviderSummary
        ) && !assembled.skipped_items.is_empty()
        {
            let available_tokens = assembled.budget.available_tokens;
            let summary_budget = crate::effective_summary_budget(
                available_tokens,
                assembled.budget.used_tokens,
                compression_options.summary_max_tokens,
            );

            if summary_budget > 0 {
                let (compressed, effective_mode) = crate::summarize_skipped_memories(
                    compression_options.mode,
                    self.llm_provider.clone(),
                    &tenant,
                    &assembled.skipped_items,
                    summary_budget,
                    format_options.format,
                    compression_options.include_summary_section,
                )
                .await;

                if !compressed.text.is_empty() {
                    assembled = apply_provider_compression_summary(
                        assembled,
                        compressed,
                        format_options.format,
                        available_tokens,
                        effective_mode,
                        &SimpleTokenEstimator,
                    );
                }
            }
        }

        Ok(BuildContextOutput {
            context: assembled.context,
            memories: assembled.memories,
            budget: assembled.budget,
            compression: assembled.compression,
            cache: ContextCacheUsage::disabled(),
        })
    }

    pub async fn list_memories(
        &self,
        input: ListMemoriesInput,
    ) -> MemcoreResult<ListMemoriesOutput> {
        validate_tenant(&input.tenant)?;
        let limit = normalize_list_limit(input.limit)?;
        let cursor = parse_optional_cursor(input.cursor)?;

        let memory_types = input.memory_type.map(|memory_type| vec![memory_type]);

        let memories = self
            .fact_store
            .search_facts(FactSearchQuery {
                tenant: input.tenant,
                memory_types,
                query_text: input.query_text,
                limit,
                cursor,
                include_deleted: input.include_deleted,
            })
            .await?;

        let page = build_page(memories, limit, |fact| PageCursor {
            last_id: fact.id.to_string(),
            last_sort_value: fact.updated_at,
        })?;

        Ok(ListMemoriesOutput {
            memories: page.items,
            next_cursor: page.next_cursor,
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
            return Err(memcore_common::MemcoreError::NotFound(
                "memory not found".to_string(),
            ));
        }

        let fact = exists.expect("fact existence checked above");

        self.fact_store
            .soft_delete_fact(&input.tenant, input.memory_id)
            .await?;

        self.vector_store
            .delete_by_fact_id(&input.tenant, input.memory_id)
            .await?;

        record_event_best_effort(
            &self.event_store,
            &input.tenant,
            build_delete_event(
                &input.tenant,
                &fact,
                &self.audit_provider_name,
                &self.audit_model_name,
                serde_json::json!({ "source": "delete_memory" }),
            ),
        )
        .await;

        self.invalidate_user_context_cache(&input.tenant).await;

        Ok(DeleteMemoryOutput { deleted: true })
    }

    pub async fn apply_retention(
        &self,
        input: ApplyRetentionInput,
    ) -> MemcoreResult<ApplyRetentionOutput> {
        validate_tenant(&input.tenant)?;

        if !input.policy.enabled {
            return Ok(ApplyRetentionOutput::zero(input.dry_run));
        }

        let mut output = ApplyRetentionOutput::zero(input.dry_run);

        if let Some(fact_days) = input.policy.fact_days_active() {
            let cutoff = Utc::now() - Duration::days(i64::from(fact_days));
            let result = self
                .fact_store
                .delete_facts_older_than(&input.tenant, cutoff, input.dry_run)
                .await?;

            output.facts_matched = result.count;
            if input.dry_run {
                output.facts_deleted = 0;
            } else {
                output.facts_deleted = result.count;
                for fact_id in result.fact_ids {
                    if let Err(err) = self
                        .vector_store
                        .delete_by_fact_id(&input.tenant, fact_id)
                        .await
                    {
                        if !matches!(err, memcore_common::MemcoreError::NotFound(_)) {
                            return Err(err);
                        }
                    }
                }
            }
        }

        if let Some(event_days) = input.policy.event_days_active() {
            if let Some(event_store) = &self.event_store {
                let cutoff = Utc::now() - Duration::days(i64::from(event_days));
                let count = event_store
                    .delete_events_older_than(&input.tenant, cutoff, input.dry_run)
                    .await?;

                output.events_matched = count;
                output.events_deleted = if input.dry_run { 0 } else { count };
            }
        }

        if !input.dry_run && output.facts_deleted > 0 {
            self.invalidate_user_context_cache(&input.tenant).await;
        }

        Ok(output)
    }

    pub async fn apply_provider_usage_retention(
        &self,
        input: ApplyProviderUsageRetentionInput,
    ) -> MemcoreResult<ApplyProviderUsageRetentionOutput> {
        validate_org_id(&input.org_id)?;

        if input.retention_days == 0 {
            return Ok(ApplyProviderUsageRetentionOutput::zero(
                input.dry_run,
                Utc::now(),
            ));
        }

        let store = self.provider_usage_store.as_ref().ok_or_else(|| {
            memcore_common::MemcoreError::ValidationError(
                "provider usage persistence is not configured".to_string(),
            )
        })?;

        let cutoff = Utc::now() - Duration::days(i64::from(input.retention_days));
        let matched = store
            .delete_usage_events_older_than(&input.org_id, cutoff, true)
            .await?;

        let deleted = if input.dry_run {
            0
        } else {
            store
                .delete_usage_events_older_than(&input.org_id, cutoff, false)
                .await?
        };

        Ok(ApplyProviderUsageRetentionOutput {
            dry_run: input.dry_run,
            matched_events: matched,
            deleted_events: deleted,
            cutoff,
        })
    }

    pub async fn get_org_summary(&self, input: OrgSummaryInput) -> MemcoreResult<OrgSummaryOutput> {
        validate_org_id(&input.org_id)?;

        let total_facts = self.fact_store.count_facts_by_org(&input.org_id).await?;
        let total_users = self.fact_store.count_users_by_org(&input.org_id).await?;
        let total_events = match &self.event_store {
            Some(store) => Some(store.count_events_by_org(&input.org_id).await?),
            None => None,
        };

        Ok(OrgSummaryOutput {
            org_id: input.org_id,
            total_users,
            total_facts,
            total_events,
        })
    }

    pub async fn list_org_users(
        &self,
        input: ListOrgUsersInput,
    ) -> MemcoreResult<ListOrgUsersOutput> {
        validate_org_id(&input.org_id)?;
        let limit = normalize_org_users_limit(input.limit)?;
        let cursor = parse_optional_cursor(input.cursor)?;

        let users = self
            .fact_store
            .list_users_by_org(OrgUserListQuery {
                org_id: input.org_id,
                limit,
                cursor,
            })
            .await?;

        let page = build_page(users, limit, |user| PageCursor {
            last_id: user.user_id.clone(),
            last_sort_value: user.last_memory_at.unwrap_or_else(Utc::now),
        })?;

        Ok(ListOrgUsersOutput {
            users: page.items,
            next_cursor: page.next_cursor,
        })
    }

    pub async fn search_org_memory_events(
        &self,
        input: SearchOrgMemoryEventsInput,
    ) -> MemcoreResult<SearchOrgMemoryEventsOutput> {
        validate_org_id(&input.org_id)?;
        let limit = normalize_org_memory_events_limit(input.limit)?;
        let cursor = parse_optional_cursor(input.cursor)?;

        let Some(event_store) = &self.event_store else {
            return Ok(SearchOrgMemoryEventsOutput {
                events: Vec::new(),
                next_cursor: None,
            });
        };

        validate_event_date_range(input.created_after, input.created_before)?;

        let mut query = OrgMemoryEventQuery::new(input.org_id, limit);
        query.user_id = input.user_id;
        query.fact_id = input.fact_id;
        query.operation = input.operation;
        query.created_after = input.created_after;
        query.created_before = input.created_before;
        query.query_text = input.query_text;
        query.cursor = cursor;

        let events = event_store.list_events_by_org(query).await?;

        let page = build_page(events, limit, |event| PageCursor {
            last_id: event.id.to_string(),
            last_sort_value: event.created_at,
        })?;

        Ok(SearchOrgMemoryEventsOutput {
            events: page.items,
            next_cursor: page.next_cursor,
        })
    }

    pub async fn get_org_quota_status(
        &self,
        input: GetOrgQuotaStatusInput,
    ) -> MemcoreResult<QuotaCheckResult> {
        validate_org_id(&input.org_id)?;
        if let Some(user_id) = &input.user_id {
            TenantContext::new(input.org_id.clone(), user_id.clone())?;
        }

        self.quota_service().get_org_quota_status(input).await
    }

    pub async fn get_org_usage_dashboard(
        &self,
        input: OrgUsageDashboardInput,
    ) -> MemcoreResult<OrgUsageDashboardOutput> {
        validate_org_id(&input.org_id)?;
        validate_event_date_range(Some(input.created_after), Some(input.created_before))?;

        let plan = match &self.org_plan_store {
            Some(store) => store.get_org_plan(&input.org_id).await?,
            None => None,
        };
        let quota = self
            .get_org_quota_status(GetOrgQuotaStatusInput {
                org_id: input.org_id.clone(),
                user_id: None,
            })
            .await?;

        let active_memories = self.fact_store.count_facts_by_org(&input.org_id).await? as u64;
        let latest_snapshot = self.latest_memory_usage_snapshot(&input.org_id).await?;
        let memory = OrgMemoryUsageSummary {
            total_users: self.fact_store.count_users_by_org(&input.org_id).await? as u64,
            total_memories: active_memories,
            active_memories,
            deleted_memories: None,
            latest_snapshot,
        };

        let provider = match &self.provider_usage_store {
            Some(store) => {
                let result = store
                    .query_usage(ProviderUsageQuery {
                        org_id: input.org_id.clone(),
                        user_id: None,
                        provider_name: None,
                        model_name: None,
                        capability: None,
                        operation_name: None,
                        created_after: Some(input.created_after),
                        created_before: Some(input.created_before),
                        limit: DEFAULT_PROVIDER_USAGE_LIMIT,
                        cursor: None,
                    })
                    .await?;
                result.summary.into()
            }
            None => empty_provider_usage_summary(),
        };

        Ok(OrgUsageDashboardOutput {
            org_id: input.org_id,
            generated_at: Utc::now(),
            window_start: input.created_after,
            window_end: input.created_before,
            plan,
            quota,
            memory,
            provider,
        })
    }

    pub async fn create_memory_usage_snapshot(
        &self,
        input: CreateMemoryUsageSnapshotInput,
    ) -> MemcoreResult<CreateMemoryUsageSnapshotOutput> {
        validate_org_id(&input.org_id)?;
        let store = self.memory_usage_snapshot_store()?;

        let active_memories = self.fact_store.count_facts_by_org(&input.org_id).await? as u64;
        let snapshot = MemoryUsageSnapshot {
            id: Uuid::new_v4(),
            org_id: input.org_id.clone(),
            total_users: self.fact_store.count_users_by_org(&input.org_id).await? as u64,
            total_memories: active_memories,
            active_memories,
            deleted_memories: None,
            captured_at: input.captured_at.unwrap_or_else(Utc::now),
            metadata: None,
        };

        let snapshot = store.insert_snapshot(snapshot).await?;
        Ok(CreateMemoryUsageSnapshotOutput { snapshot })
    }

    pub async fn query_memory_usage_snapshots(
        &self,
        input: QueryMemoryUsageSnapshotsInput,
    ) -> MemcoreResult<QueryMemoryUsageSnapshotsOutput> {
        validate_org_id(&input.org_id)?;
        validate_event_date_range(input.created_after, input.created_before)?;
        let limit = validate_memory_usage_snapshot_limit(input.limit)?;
        let store = self.memory_usage_snapshot_store()?;

        let result = store
            .query_snapshots(MemoryUsageSnapshotQuery {
                org_id: input.org_id,
                created_after: input.created_after,
                created_before: input.created_before,
                limit,
                cursor: input.cursor,
            })
            .await?;

        Ok(QueryMemoryUsageSnapshotsOutput {
            snapshots: result.snapshots,
            next_cursor: result.next_cursor,
        })
    }

    async fn latest_memory_usage_snapshot(
        &self,
        org_id: &str,
    ) -> MemcoreResult<Option<MemoryUsageLatestSnapshot>> {
        let Some(store) = &self.memory_usage_snapshot_store else {
            return Ok(None);
        };

        let result = store
            .query_snapshots(MemoryUsageSnapshotQuery {
                org_id: org_id.to_string(),
                created_after: None,
                created_before: None,
                limit: 1,
                cursor: None,
            })
            .await?;

        Ok(result
            .snapshots
            .first()
            .map(MemoryUsageLatestSnapshot::from))
    }

    pub async fn get_provider_usage_daily(
        &self,
        input: ProviderUsageDailyInput,
    ) -> MemcoreResult<ProviderUsageDailyOutput> {
        validate_org_id(&input.org_id)?;
        validate_event_date_range(Some(input.created_after), Some(input.created_before))?;

        let buckets = match &self.provider_usage_store {
            Some(store) => {
                let mut query = ProviderUsageDailyQuery::new(
                    input.org_id.clone(),
                    input.created_after,
                    input.created_before,
                );
                query.provider_name = input.provider_name;
                query.model_name = input.model_name;
                query.capability = input.capability;
                store.query_usage_daily(query).await?
            }
            None => Vec::new(),
        };

        Ok(ProviderUsageDailyOutput {
            org_id: input.org_id,
            window_start: input.created_after,
            window_end: input.created_before,
            buckets,
        })
    }

    pub async fn resolve_org_quota_limits(
        &self,
        org_id: &str,
    ) -> MemcoreResult<ResolvedOrgQuotaLimits> {
        validate_org_id(org_id)?;
        self.quota_service().resolve_org_quota_limits(org_id).await
    }

    pub async fn check_memory_write_quota(
        &self,
        input: CheckMemoryWriteQuotaInput,
    ) -> MemcoreResult<QuotaCheckResult> {
        let tenant = TenantContext::new(input.org_id.clone(), input.user_id.clone())?;
        validate_tenant(&tenant)?;
        self.quota_service().check_memory_write_allowed(input).await
    }

    pub async fn check_provider_quota(
        &self,
        input: CheckProviderQuotaInput,
    ) -> MemcoreResult<QuotaCheckResult> {
        validate_org_id(&input.org_id)?;
        self.quota_service()
            .check_provider_call_allowed(input)
            .await
    }

    fn quota_service(&self) -> QuotaService {
        QuotaService::new(
            self.fact_store.clone(),
            self.provider_usage_store.clone(),
            self.org_plan_store.clone(),
            self.global_quota_limits.clone(),
        )
    }

    pub async fn forget_user(&self, input: ForgetUserInput) -> MemcoreResult<ForgetUserOutput> {
        validate_tenant(&input.tenant)?;

        self.fact_store.delete_user_data(&input.tenant).await?;

        self.vector_store.delete_by_user(&input.tenant).await?;

        record_event_best_effort(
            &self.event_store,
            &input.tenant,
            build_forget_user_event(
                &input.tenant,
                &self.audit_provider_name,
                &self.audit_model_name,
            ),
        )
        .await;

        self.invalidate_user_context_cache(&input.tenant).await;

        Ok(ForgetUserOutput { deleted: true })
    }

    pub async fn import_user_data(
        &self,
        input: ImportUserDataInput,
    ) -> MemcoreResult<ImportUserDataOutput> {
        validate_tenant(&input.tenant)?;

        let validation =
            collect_import_validation(&input.export, &input.tenant, input.restore_events);

        if !validation.valid {
            if input.dry_run {
                return Ok(dry_run_output(
                    &input,
                    validation,
                    self.event_store.is_some(),
                ));
            }
            return Err(memcore_common::MemcoreError::ValidationError(
                validation
                    .first_error_message()
                    .unwrap_or_else(|| "import validation failed".to_string()),
            ));
        }

        if input.dry_run {
            return Ok(dry_run_output(
                &input,
                validation,
                self.event_store.is_some(),
            ));
        }

        let mut replaced_existing = false;

        if matches!(input.mode, ImportMode::Replace) {
            self.fact_store.delete_user_data(&input.tenant).await?;
            self.vector_store.delete_by_user(&input.tenant).await?;
            replaced_existing = true;

            record_event_best_effort(
                &self.event_store,
                &input.tenant,
                build_import_replace_event(
                    &input.tenant,
                    &self.audit_provider_name,
                    &self.audit_model_name,
                ),
            )
            .await;
        }

        let mut imported_facts = 0usize;
        let skipped_facts = 0usize;

        for exported_fact in &input.export.facts {
            let id_exists = self
                .fact_store
                .get_fact(&input.tenant, exported_fact.id)
                .await?
                .is_some();

            let fact_id = resolve_import_fact_id(exported_fact.id, id_exists, input.mode);
            let fact = fact_for_import(exported_fact, &input.tenant, fact_id)?;

            self.fact_store
                .insert_fact(&input.tenant, fact.clone())
                .await?;

            let embedding = self.embedding_provider.embed_text(&fact.content).await?;

            let record = VectorRecord {
                id: Uuid::new_v4(),
                fact_id: fact.id,
                org_id: fact.org_id.clone(),
                user_id: fact.user_id.clone(),
                embedding,
                content: fact.content.clone(),
                memory_type: fact.memory_type,
                metadata: fact.metadata.clone(),
            };

            self.vector_store
                .upsert_vector(&input.tenant, record)
                .await?;

            imported_facts += 1;
        }

        let mut imported_events = 0usize;

        if input.restore_events {
            if let Some(event_store) = &self.event_store {
                for exported_event in &input.export.memory_events {
                    let event = restored_event_from_export(exported_event);
                    event_store.record_event(&input.tenant, event).await?;
                    imported_events += 1;
                }
            }
        }

        self.invalidate_user_context_cache(&input.tenant).await;

        Ok(ImportUserDataOutput {
            imported_facts,
            imported_events,
            skipped_facts,
            replaced_existing,
            dry_run: false,
            validation,
        })
    }

    pub async fn export_user_data(
        &self,
        input: ExportUserDataInput,
    ) -> MemcoreResult<UserMemoryExport> {
        validate_tenant(&input.tenant)?;

        let facts = self
            .fact_store
            .search_facts(FactSearchQuery {
                tenant: input.tenant.clone(),
                memory_types: None,
                query_text: None,
                limit: EXPORT_FACTS_LIMIT,
                cursor: None,
                include_deleted: input.include_deleted,
            })
            .await?;

        let memory_events = if input.include_events {
            match &self.event_store {
                Some(event_store) => {
                    event_store
                        .list_events(MemoryEventQuery::new(
                            input.tenant.clone(),
                            EXPORT_EVENTS_LIMIT,
                        ))
                        .await?
                }
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };

        Ok(UserMemoryExport::new(
            input.tenant.org_id,
            input.tenant.user_id,
            facts,
            memory_events,
        ))
    }

    pub async fn list_memory_events(
        &self,
        input: ListMemoryEventsInput,
    ) -> MemcoreResult<ListMemoryEventsOutput> {
        validate_tenant(&input.tenant)?;
        let limit = normalize_memory_event_list_limit(input.limit)?;
        let cursor = parse_optional_cursor(input.cursor)?;

        let Some(event_store) = &self.event_store else {
            return Ok(ListMemoryEventsOutput {
                events: Vec::new(),
                next_cursor: None,
            });
        };

        validate_event_date_range(input.created_after, input.created_before)?;

        let mut query = MemoryEventQuery::new(input.tenant, limit);
        query.fact_id = input.fact_id;
        query.operation = input.operation;
        query.created_after = input.created_after;
        query.created_before = input.created_before;
        query.query_text = input.query_text;
        query.cursor = cursor;

        let events = event_store.list_events(query).await?;

        let page = build_page(events, limit, |event| PageCursor {
            last_id: event.id.to_string(),
            last_sort_value: event.created_at,
        })?;

        Ok(ListMemoryEventsOutput {
            events: page.items,
            next_cursor: page.next_cursor,
        })
    }

    async fn invalidate_user_context_cache(&self, tenant: &TenantContext) {
        if self.context_cache_config.enabled {
            match self
                .context_cache
                .invalidate_user(&tenant.org_id, &tenant.user_id)
                .await
            {
                Ok(removed) => {
                    self.context_cache_metrics.record_invalidation(removed);
                    tracing::debug!(
                        event = "context_cache_invalidated",
                        org_id = %tenant.org_id,
                        user_id = %tenant.user_id,
                        removed_entries = removed,
                        "context cache invalidated for user"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        event = "context_cache_invalidation_failed",
                        org_id = %tenant.org_id,
                        user_id = %tenant.user_id,
                        error_code = %error.code(),
                        "context cache invalidation failed"
                    );
                }
            }
        }
    }

    fn memory_usage_snapshot_store(&self) -> MemcoreResult<&Arc<dyn MemoryUsageSnapshotStore>> {
        self.memory_usage_snapshot_store.as_ref().ok_or_else(|| {
            memcore_common::MemcoreError::StorageError(
                "memory usage snapshot store is not configured".to_string(),
            )
        })
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
    use memcore_common::MemcoreError;

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

fn validate_org_id(org_id: &str) -> MemcoreResult<()> {
    use memcore_common::MemcoreError;

    if org_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "org_id cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn normalize_org_users_limit(limit: usize) -> MemcoreResult<usize> {
    use memcore_common::MemcoreError;

    let normalized = if limit == 0 {
        DEFAULT_LIST_ORG_USERS_LIMIT
    } else {
        limit
    };

    if normalized > MAX_LIST_ORG_USERS_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_LIST_ORG_USERS_LIMIT}"
        )));
    }

    Ok(normalized)
}

fn normalize_org_memory_events_limit(limit: usize) -> MemcoreResult<usize> {
    use memcore_common::MemcoreError;

    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT}"
        )));
    }

    Ok(limit)
}

fn validate_query(query: &str) -> MemcoreResult<()> {
    use memcore_common::MemcoreError;

    if query.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "query cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn normalize_context_max_memories(max_memories: usize) -> MemcoreResult<usize> {
    use memcore_common::MemcoreError;

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
    use memcore_common::MemcoreError;

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
    use memcore_common::MemcoreError;

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

/// Per-source retrieval limit before RRF fusion (2× final limit, capped at max search limit).
fn internal_search_limit(limit: usize) -> usize {
    limit.saturating_mul(2).min(types::MAX_SEARCH_LIMIT)
}

fn normalize_memory_event_list_limit(limit: usize) -> MemcoreResult<usize> {
    use memcore_common::MemcoreError;

    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > types::MAX_LIST_MEMORY_EVENTS_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {}",
            types::MAX_LIST_MEMORY_EVENTS_LIMIT
        )));
    }

    Ok(limit)
}

fn validate_messages(messages: &[crate::ports::MemoryMessage]) -> MemcoreResult<()> {
    use memcore_common::MemcoreError;

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

fn validate_candidate(candidate: &crate::CandidateFact) -> MemcoreResult<()> {
    use memcore_common::MemcoreError;

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

fn passes_importance_threshold(candidate: &crate::CandidateFact, min_importance: f32) -> bool {
    candidate.importance >= min_importance
}

fn dedup_noop_decision(existing_fact_id: Uuid, reason: String) -> FactOperationDecision {
    FactOperationDecision {
        operation: FactOperation::NoOp,
        target_fact_id: Some(existing_fact_id),
        reason: Some(reason),
        confidence: 1.0,
    }
}

fn fact_for_import(exported: &Fact, tenant: &TenantContext, fact_id: Uuid) -> MemcoreResult<Fact> {
    Fact::new(
        fact_id,
        tenant.org_id.clone(),
        tenant.user_id.clone(),
        exported.memory_type,
        exported.content.clone(),
        exported.summary.clone(),
        exported.source,
        exported.confidence,
        exported.importance,
        exported.valid_at,
        exported.invalid_at,
        exported.recorded_at,
        exported.updated_at,
        exported.metadata.clone(),
    )
}

fn restored_event_from_export(exported: &MemoryEvent) -> MemoryEvent {
    MemoryEvent {
        id: Uuid::new_v4(),
        org_id: exported.org_id.clone(),
        user_id: exported.user_id.clone(),
        fact_id: exported.fact_id,
        operation: exported.operation,
        input_text: None,
        previous_content: exported.previous_content.clone(),
        new_content: exported.new_content.clone(),
        provider_name: exported.provider_name.clone(),
        model_name: exported.model_name.clone(),
        metadata: exported.metadata.clone(),
        created_at: exported.created_at,
    }
}

fn dry_run_output(
    input: &ImportUserDataInput,
    validation: ImportValidationSummary,
    event_store_configured: bool,
) -> ImportUserDataOutput {
    let replaced_existing = matches!(input.mode, ImportMode::Replace);

    if !validation.valid {
        return ImportUserDataOutput {
            imported_facts: 0,
            imported_events: 0,
            skipped_facts: input.export.facts.len(),
            replaced_existing,
            dry_run: true,
            validation,
        };
    }

    let imported_events = if input.restore_events && event_store_configured {
        input.export.memory_events.len()
    } else {
        0
    };

    ImportUserDataOutput {
        imported_facts: input.export.facts.len(),
        imported_events,
        skipped_facts: 0,
        replaced_existing,
        dry_run: true,
        validation,
    }
}
