mod detector;
mod embedding;
mod types;

pub use detector::{
    detect_duplicate, find_existing_facts_for_dedup, normalize_content, token_overlap_ratio,
};
pub use embedding::detect_embedding_duplicate;
pub use types::{
    DeduplicationDecision, EmbeddingDeduplicationConfig, DEDUP_SEARCH_LIMIT,
    DEFAULT_EMBEDDING_DEDUP_SEARCH_LIMIT, DEFAULT_EMBEDDING_DEDUP_SIMILARITY_THRESHOLD,
    EXACT_DUPLICATE_THRESHOLD, HIGH_SIMILARITY_DUPLICATE_THRESHOLD,
    MODERATE_SIMILARITY_THRESHOLD,
};
