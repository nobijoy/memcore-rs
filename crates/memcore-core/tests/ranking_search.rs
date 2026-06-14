use std::sync::Arc;

use chrono::{Duration, Utc};
use memcore_core::{
    BuildContextInput, Fact, FactStore, MemoryEngine, MemorySearchResult, MemorySource,
    MemoryType, RankingConfig, SearchMemoryInput, TenantContext, VectorRecord, VectorStore,
};
use memcore_providers::{deterministic_embedding, MockEmbeddingProvider, MockLlmProvider};
use memcore_storage::{MockFactStore, MockVectorStore};
use serde_json::json;
use uuid::Uuid;

fn tenant(org_id: &str, user_id: &str) -> TenantContext {
    TenantContext::new(org_id, user_id).expect("tenant")
}

fn engine_with(fact_store: Arc<MockFactStore>, vector_store: Arc<MockVectorStore>) -> MemoryEngine {
    MemoryEngine::new(
        fact_store,
        vector_store,
        Arc::new(MockLlmProvider::new()),
        Arc::new(MockEmbeddingProvider::new(8)),
    )
}

fn embedding_with_cosine_similarity(reference: &[f32], similarity: f32) -> Vec<f32> {
    let dim = reference.len();
    let ref_norm: f32 = reference.iter().map(|x| x * x).sum::<f32>().sqrt();
    let ref_unit: Vec<f32> = reference.iter().map(|x| x / ref_norm).collect();

    let mut orthogonal = vec![0.0_f32; dim];
    orthogonal[dim.saturating_sub(1)] = 1.0;
    let proj: f32 = orthogonal
        .iter()
        .zip(ref_unit.iter())
        .map(|(a, b)| a * b)
        .sum();
    for (o, r) in orthogonal.iter_mut().zip(ref_unit.iter()) {
        *o -= proj * r;
    }
    let orth_norm: f32 = orthogonal.iter().map(|x| x * x).sum::<f32>().sqrt();
    if orth_norm > 0.0 {
        for v in &mut orthogonal {
            *v /= orth_norm;
        }
    }

    let sin_theta = (1.0 - similarity * similarity).max(0.0).sqrt();
    ref_unit
        .iter()
        .zip(orthogonal.iter())
        .map(|(r, o)| similarity * r + sin_theta * o)
        .collect()
}

async fn insert_scored_fact(
    fact_store: &MockFactStore,
    vector_store: &MockVectorStore,
    org_id: &str,
    user_id: &str,
    content: &str,
    importance: f32,
    confidence: f32,
    updated_at: chrono::DateTime<Utc>,
    memory_type: MemoryType,
    embedding: Vec<f32>,
) -> Fact {
    let fact = Fact::new(
        Uuid::new_v4(),
        org_id,
        user_id,
        memory_type,
        content,
        None,
        MemorySource::UserMessage,
        confidence,
        importance,
        None,
        None,
        updated_at,
        updated_at,
        json!({}),
    )
    .expect("fact");

    fact_store
        .insert_fact(&tenant(org_id, user_id), fact.clone())
        .await
        .expect("insert fact");

    vector_store
        .upsert_vector(
            &tenant(org_id, user_id),
            VectorRecord {
                id: Uuid::new_v4(),
                fact_id: fact.id,
                org_id: org_id.to_string(),
                user_id: user_id.to_string(),
                embedding,
                content: content.to_string(),
                memory_type,
                metadata: json!({}),
            },
        )
        .await
        .expect("upsert vector");

    fact
}

#[tokio::test]
async fn search_results_are_sorted_by_final_score_descending() {
    let fact_store = Arc::new(MockFactStore::new());
    let vector_store = Arc::new(MockVectorStore::new());
    let query_embedding = deterministic_embedding("ranking search query alpha", 8).expect("embed");

    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "first ranked memory alpha bravo",
        0.7,
        0.8,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.75),
    )
    .await;
    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "second ranked memory charlie delta",
        0.9,
        0.85,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.70),
    )
    .await;
    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "third ranked memory echo foxtrot",
        0.5,
        0.6,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.95),
    )
    .await;

    let output = engine_with(fact_store, vector_store)
        .search_memory(SearchMemoryInput {
            tenant: tenant("org_rank", "user_a"),
            query: "ranking search query alpha".to_string(),
            limit: 10,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search");

    assert_eq!(output.results.len(), 3);
    for window in output.results.windows(2) {
        assert!(window[0].score >= window[1].score);
    }
}

#[tokio::test]
async fn high_importance_can_outrank_slightly_higher_semantic_score() {
    let fact_store = Arc::new(MockFactStore::new());
    let vector_store = Arc::new(MockVectorStore::new());
    let query_embedding = deterministic_embedding("importance outrank query", 8).expect("embed");

    let high_importance = insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "high importance memory alpha bravo",
        0.95,
        0.8,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.82),
    )
    .await;
    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "lower importance memory charlie delta",
        0.5,
        0.8,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.88),
    )
    .await;

    let output = engine_with(fact_store, vector_store)
        .search_memory(SearchMemoryInput {
            tenant: tenant("org_rank", "user_a"),
            query: "importance outrank query".to_string(),
            limit: 10,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search");

    assert_eq!(output.results.len(), 2);
    assert_eq!(output.results[0].fact_id, high_importance.id);
}

#[tokio::test]
async fn low_confidence_result_is_ranked_lower() {
    let fact_store = Arc::new(MockFactStore::new());
    let vector_store = Arc::new(MockVectorStore::new());
    let query_embedding = deterministic_embedding("confidence ranking query", 8).expect("embed");

    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "low confidence memory alpha bravo",
        0.8,
        0.3,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.85),
    )
    .await;
    let high_confidence = insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "high confidence memory charlie delta",
        0.8,
        0.95,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.85),
    )
    .await;

    let output = engine_with(fact_store, vector_store)
        .search_memory(SearchMemoryInput {
            tenant: tenant("org_rank", "user_a"),
            query: "confidence ranking query".to_string(),
            limit: 10,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search");

    assert_eq!(output.results[0].fact_id, high_confidence.id);
}

#[tokio::test]
async fn older_fact_gets_lower_rank_than_recent_fact_with_same_signals() {
    let fact_store = Arc::new(MockFactStore::new());
    let vector_store = Arc::new(MockVectorStore::new());
    let query_embedding = deterministic_embedding("freshness ranking query", 8).expect("embed");
    let now = Utc::now();

    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "old memory alpha bravo charlie",
        0.8,
        0.8,
        now - Duration::days(400),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.9),
    )
    .await;
    let recent = insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "recent memory delta echo foxtrot",
        0.8,
        0.8,
        now - Duration::days(2),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.9),
    )
    .await;

    let output = engine_with(fact_store, vector_store)
        .search_memory(SearchMemoryInput {
            tenant: tenant("org_rank", "user_a"),
            query: "freshness ranking query".to_string(),
            limit: 10,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search");

    assert_eq!(output.results[0].fact_id, recent.id);
}

#[tokio::test]
async fn context_uses_ranked_search_ordering() {
    let fact_store = Arc::new(MockFactStore::new());
    let vector_store = Arc::new(MockVectorStore::new());
    let query_embedding = deterministic_embedding("context ranking query", 8).expect("embed");

    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "LOWER_RANKED_MEMORY_CONTENT",
        0.5,
        0.8,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.88),
    )
    .await;
    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_rank",
        "user_a",
        "HIGHER_RANKED_MEMORY_CONTENT",
        0.95,
        0.8,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.82),
    )
    .await;

    let output = engine_with(fact_store, vector_store)
        .build_context(BuildContextInput {
            tenant: tenant("org_rank", "user_a"),
            query: "context ranking query".to_string(),
            max_memories: 10,
            memory_types: None,
            include_metadata: false,
        })
        .await
        .expect("build context");

    let higher_pos = output
        .context
        .find("HIGHER_RANKED_MEMORY_CONTENT")
        .expect("higher ranked content");
    let lower_pos = output
        .context
        .find("LOWER_RANKED_MEMORY_CONTENT")
        .expect("lower ranked content");
    assert!(higher_pos < lower_pos);
    assert_eq!(output.memories[0].content, "HIGHER_RANKED_MEMORY_CONTENT");
}

#[tokio::test]
async fn search_preserves_tenant_isolation_with_ranking() {
    let fact_store = Arc::new(MockFactStore::new());
    let vector_store = Arc::new(MockVectorStore::new());
    let query_embedding = deterministic_embedding("tenant ranking query", 8).expect("embed");

    insert_scored_fact(
        &fact_store,
        &vector_store,
        "org_a",
        "user_a",
        "org a memory alpha bravo",
        0.9,
        0.9,
        Utc::now(),
        MemoryType::Skill,
        embedding_with_cosine_similarity(&query_embedding, 0.95),
    )
    .await;

    let output = engine_with(fact_store, vector_store)
        .search_memory(SearchMemoryInput {
            tenant: tenant("org_b", "user_a"),
            query: "tenant ranking query".to_string(),
            limit: 10,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search");

    assert!(output.results.is_empty());
}

#[test]
fn apply_ranking_sorts_in_memory_results() {
    use memcore_core::apply_ranking;

    let now = Utc::now();
    let mut results = vec![
        MemorySearchResult {
            fact_id: Uuid::new_v4(),
            content: "low".to_string(),
            memory_type: MemoryType::Conversation,
            score: 0.9,
            confidence: 0.5,
            importance: 0.5,
            valid_at: None,
            metadata: json!({}),
        },
        MemorySearchResult {
            fact_id: Uuid::new_v4(),
            content: "high".to_string(),
            memory_type: MemoryType::Profile,
            score: 0.7,
            confidence: 0.9,
            importance: 0.95,
            valid_at: None,
            metadata: json!({}),
        },
    ];

    apply_ranking(
        &mut results,
        |_| Some(now),
        now,
        &RankingConfig::default(),
    );

    assert_eq!(results[0].content, "high");
    assert!(results[0].score >= results[1].score);
}
