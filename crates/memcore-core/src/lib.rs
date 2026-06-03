pub mod engine;
pub mod models;
pub mod ports;

pub use engine::{
    AddMemoryInput, AddMemoryOutput, MemoryEngine, MemoryOperationSummary, DEFAULT_MIN_IMPORTANCE,
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
