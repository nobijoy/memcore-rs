pub mod context;
pub mod engine;
pub mod models;
pub mod ports;
pub mod privacy;

pub use context::{
    assemble_context, BuildContextInput, BuildContextOutput, DEFAULT_CONTEXT_MAX_MEMORIES,
    EMPTY_CONTEXT_MESSAGE, MAX_CONTEXT_MAX_MEMORIES,
};
pub use engine::{
    AddMemoryInput, AddMemoryOutput, ListMemoriesInput, ListMemoriesOutput, MemoryEngine,
    MemoryOperationSummary, SearchMemoryInput, SearchMemoryOutput, DEFAULT_LIST_MEMORIES_LIMIT,
    DEFAULT_MIN_IMPORTANCE, DEFAULT_SEARCH_LIMIT, MAX_LIST_MEMORIES_LIMIT, MAX_SEARCH_LIMIT,
};
pub use models::{
    CandidateFact, Fact, FactOperation, FactOperationDecision, MemorySearchResult, MemorySource,
    MemoryType, TenantContext,
};
pub use privacy::PiiRedactor;
pub use ports::{
    EmbeddingProvider, FactClassificationInput, FactExtractionInput, FactSearchQuery, FactStore,
    LlmProvider, MemoryMessage, MessageRole, SummarizationInput, VectorRecord, VectorSearchQuery,
    VectorSearchResult, VectorStore,
};
