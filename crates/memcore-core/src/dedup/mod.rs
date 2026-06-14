mod detector;
mod types;

pub use detector::{
    detect_duplicate, find_existing_facts_for_dedup, normalize_content, token_overlap_ratio,
};
pub use types::{
    DeduplicationDecision, DEDUP_SEARCH_LIMIT, EXACT_DUPLICATE_THRESHOLD,
    HIGH_SIMILARITY_DUPLICATE_THRESHOLD, MODERATE_SIMILARITY_THRESHOLD,
};
