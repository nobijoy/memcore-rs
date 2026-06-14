use memcore_common::MemcoreResult;

use crate::ports::{VectorSearchQuery, VectorStore};
use crate::{MemoryType, TenantContext};

use super::types::{DeduplicationDecision, EmbeddingDeduplicationConfig};

/// Checks vector store for semantically similar facts above the configured threshold.
///
/// Search scores are treated as **cosine similarity** in \[0.0, 1.0\] (higher = more similar).
/// A match is a duplicate when `score >= similarity_threshold`.
pub async fn detect_embedding_duplicate(
    vector_store: &dyn VectorStore,
    tenant: &TenantContext,
    memory_type: MemoryType,
    embedding: &[f32],
    config: &EmbeddingDeduplicationConfig,
) -> MemcoreResult<Option<DeduplicationDecision>> {
    if !config.enabled {
        return Ok(None);
    }

    let results = vector_store
        .search_vectors(VectorSearchQuery {
            tenant: tenant.clone(),
            embedding: embedding.to_vec(),
            limit: config.search_limit,
            memory_types: Some(vec![memory_type]),
            metadata_filter: None,
        })
        .await?;

    for result in results {
        if result.score >= config.similarity_threshold {
            return Ok(Some(DeduplicationDecision::Duplicate {
                existing_fact_id: result.fact_id,
                reason: format!(
                    "embedding similarity ({:.2} >= {:.2})",
                    result.score, config.similarity_threshold
                ),
            }));
        }
    }

    Ok(None)
}
