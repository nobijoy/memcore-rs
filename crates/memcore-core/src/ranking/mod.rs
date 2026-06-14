mod scorer;
mod types;

use chrono::{DateTime, Utc};
use std::cmp::Ordering;

use crate::MemorySearchResult;

pub use scorer::{clamp01, freshness_score, memory_type_boost, MemoryRanker};
pub use types::RankingConfig;

/// Applies weighted ranking to search results and sorts by final score descending.
///
/// `updated_at_for` supplies fact timestamps for freshness; when missing, `now` is used
/// (recent freshness). Each result's `score` field is replaced with the final ranking score.
/// Semantic similarity is read from the incoming `score` before replacement.
///
/// Known limitation: callers may fetch facts individually (N+1); batch fetch is future work.
pub fn apply_ranking(
    results: &mut [MemorySearchResult],
    updated_at_for: impl Fn(uuid::Uuid) -> Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    config: &RankingConfig,
) {
    for result in results.iter_mut() {
        let semantic_score = clamp01(result.score);
        let timestamp = updated_at_for(result.fact_id).unwrap_or(now);
        let fresh = freshness_score(timestamp, now);
        let type_boost = memory_type_boost(&result.memory_type);

        result.score = MemoryRanker::score(
            semantic_score,
            result.importance,
            result.confidence,
            fresh,
            type_boost,
            config,
        );
    }

    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
    });
}
