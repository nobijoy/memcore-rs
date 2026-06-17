pub mod admin;
pub mod audit;
pub mod context;
pub mod dedup;
pub mod engine;
pub mod importance;
pub mod pagination;
pub mod export;
pub mod import;
pub mod ranking;
pub mod retention;
pub mod lifecycle;
pub mod models;
pub mod ports;
pub mod privacy;

pub use dedup::{
    detect_duplicate, detect_embedding_duplicate, find_existing_facts_for_dedup,
    normalize_content, DeduplicationDecision, EmbeddingDeduplicationConfig,
    DEFAULT_EMBEDDING_DEDUP_SEARCH_LIMIT, DEFAULT_EMBEDDING_DEDUP_SIMILARITY_THRESHOLD,
    EXACT_DUPLICATE_THRESHOLD, HIGH_SIMILARITY_DUPLICATE_THRESHOLD,
    MODERATE_SIMILARITY_THRESHOLD,
};
pub use importance::ImportanceScorer;
pub use context::{
    assemble_context, assemble_context_with_budget, apply_provider_compression_summary,
    AssembledContext, BuildContextInput, BuildContextOutput, ContextBudget, ContextBudgetUsage,
    ContextCompressionMode, ContextCompressionOptions, ContextCompressionUsage, ContextCache,
    ContextCacheConfig, ContextCacheCoordinator, ContextCacheKey, ContextCacheMetricsRecorder,
    ContextCacheMetricsSnapshot, ContextCacheStampedeConfig, ContextCacheUsage, ContextFormat,
    context_cache_metrics_recorder, InMemoryContextCacheMetrics, NoopContextCacheMetrics,
    ContextFormatOptions, ContextFormatter, ContextMemoryItem, ContextSummarizer,
    CachedContextEntry, DEFAULT_CONTEXT_CACHE_LOCK_TIMEOUT_SECONDS,
    DEFAULT_CONTEXT_CACHE_MAX_ENTRIES, DEFAULT_CONTEXT_CACHE_TTL_SECONDS,
    DEFAULT_CONTEXT_MAX_MEMORIES, DEFAULT_CONTEXT_MAX_TOKENS, DEFAULT_CONTEXT_RESERVED_TOKENS,
    DEFAULT_SUMMARY_MAX_TOKENS, EMPTY_CONTEXT_MESSAGE, FormattedContext, InMemoryContextCache,
    LlmContextSummarizer, MAX_CONTEXT_MAX_MEMORIES, MAX_CONTEXT_MAX_TOKENS, MAX_SUMMARY_MAX_TOKENS,
    SimpleContextCompressor, SimpleContextSummarizer, SimpleTokenEstimator, TokenEstimator,
    build_context_cache_key, cached_entry_from_output, cached_entry_with_ttl,
    effective_summary_budget,
    stable_sha256_hex, summarize_skipped_memories,
};
pub use engine::{
    AddMemoryInput, AddMemoryOutput, DeleteMemoryInput, DeleteMemoryOutput, ExportUserDataInput,
    ForgetUserInput, ForgetUserOutput, ListMemoriesInput, ListMemoriesOutput,
    ListMemoryEventsInput, ListMemoryEventsOutput, MemoryEngine, MemoryOperationSummary,
    SearchMemoryInput, SearchMemoryOutput, DEFAULT_LIST_MEMORIES_LIMIT,
    DEFAULT_LIST_MEMORY_EVENTS_LIMIT, DEFAULT_MIN_IMPORTANCE, DEFAULT_SEARCH_LIMIT,
    MAX_LIST_MEMORIES_LIMIT, MAX_LIST_MEMORY_EVENTS_LIMIT, MAX_SEARCH_LIMIT,
};
pub use export::{
    UserMemoryExport, EXPORT_EVENTS_LIMIT, EXPORT_FACTS_LIMIT, USER_EXPORT_FORMAT_VERSION,
};
pub use import::{
    ImportMode, ImportUserDataInput, ImportUserDataOutput, ImportValidationIssue,
    ImportValidationSummary,
};
pub use ranking::{
    apply_ranking, clamp01, freshness_score, memory_type_boost, reciprocal_rank_fusion,
    weighted_score_for, MemoryRanker, RankedCandidate, RankingConfig, RankingSource, RrfConfig,
};
pub use retention::{
    ApplyRetentionInput, ApplyRetentionOutput, RetentionPolicy,
};
pub use admin::{
    ListOrgUsersInput, ListOrgUsersOutput, OrgSummaryInput, OrgSummaryOutput,
    SearchOrgMemoryEventsInput, SearchOrgMemoryEventsOutput,
    DEFAULT_LIST_ORG_USERS_LIMIT, DEFAULT_SEARCH_ORG_MEMORY_EVENTS_LIMIT,
    MAX_LIST_ORG_USERS_LIMIT, MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT,
};
pub use models::{
    ApiKeyRecord, ApiKeyScope, CandidateFact, Fact, FactOperation, FactOperationDecision,
    MemoryEvent, MemoryEventOperation, MemorySearchResult, MemorySource, MemoryType, TenantContext,
};
pub use privacy::PiiRedactor;
pub use pagination::{
    build_page, decode_cursor, encode_cursor, is_after_cursor_in_desc_order, page_fetch_limit,
    parse_optional_cursor, Page, PageCursor,
};
pub use ports::{
    ApiKeyListQuery, ApiKeyStore, EmbeddingProvider, FactClassificationInput, FactExtractionInput,
    FactSearchQuery,
    FactStore, LlmProvider, MemoryEventQuery, MemoryEventStore, MemoryMessage, MessageRole,
    OrgUserListQuery, OrgUserSummary, OrgMemoryEventQuery, RetentionPruneResult, SummarizationInput, VectorRecord, VectorSearchQuery, VectorSearchResult,
    VectorStore, validate_event_date_range,
    DEFAULT_MEMORY_EVENT_LIST_LIMIT, MAX_MEMORY_EVENT_LIST_LIMIT,
};
