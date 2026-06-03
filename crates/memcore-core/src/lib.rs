pub mod engine;
pub mod models;
pub mod ports;

pub use engine::{
    AddMemoryInput, AddMemoryOutput, MemoryEngine, MemoryOperationSummary, SearchMemoryInput,
    SearchMemoryOutput, DEFAULT_MIN_IMPORTANCE, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT,
};
pub use models::{
    CandidateFact, Fact, FactOperation, FactOperationDecision, MemorySearchResult, MemorySource,
    MemoryType, TenantContext,
};
pub use ports::{
    EmbeddingProvider, FactClassificationInput, FactExtractionInput, FactSearchQuery, FactStore,
    LlmProvider, MemoryMessage, MessageRole, SummarizationInput, VectorRecord, VectorSearchQuery,
    VectorSearchResult, VectorStore,
};
