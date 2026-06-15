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
    assemble_context, assemble_context_with_budget, AssembledContext, BuildContextInput,
    BuildContextOutput, ContextBudget, ContextBudgetUsage, ContextFormat, ContextFormatOptions,
    ContextFormatter, ContextMemoryItem, DEFAULT_CONTEXT_MAX_MEMORIES, DEFAULT_CONTEXT_MAX_TOKENS,
    DEFAULT_CONTEXT_RESERVED_TOKENS, EMPTY_CONTEXT_MESSAGE, FormattedContext, MAX_CONTEXT_MAX_MEMORIES,
    MAX_CONTEXT_MAX_TOKENS, SimpleTokenEstimator, TokenEstimator,
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
