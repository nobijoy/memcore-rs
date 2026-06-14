use uuid::Uuid;

/// Normalized content match (threshold 1.0).
pub const EXACT_DUPLICATE_THRESHOLD: f32 = 1.0;

/// Token-overlap ratio at or above which a candidate is treated as a duplicate.
pub const HIGH_SIMILARITY_DUPLICATE_THRESHOLD: f32 = 0.90;

/// Token-overlap ratio at or above which classification may decide Update vs Add.
pub const MODERATE_SIMILARITY_THRESHOLD: f32 = 0.65;

/// Maximum active facts scanned per deduplication check (same tenant + memory type).
pub const DEDUP_SEARCH_LIMIT: usize = 50;

#[derive(Debug, Clone, PartialEq)]
pub enum DeduplicationDecision {
    Duplicate {
        existing_fact_id: Uuid,
        reason: String,
    },
    SimilarButDistinct {
        reason: String,
    },
    New,
}
