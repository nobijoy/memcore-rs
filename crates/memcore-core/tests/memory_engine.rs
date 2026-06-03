use std::sync::Arc;

use memcore_common::MemcoreError;
use memcore_core::{
    AddMemoryInput, BuildContextInput, CandidateFact, FactStore, MemoryEngine, MemoryMessage,
    MemoryType, MessageRole, SearchMemoryInput, TenantContext, VectorSearchQuery, VectorStore,
    EMPTY_CONTEXT_MESSAGE,
};
use memcore_providers::{deterministic_embedding, MockEmbeddingProvider, MockLlmProvider};
use memcore_storage::{MockFactStore, MockVectorStore};
use serde_json::json;

fn tenant(org_id: &str, user_id: &str) -> TenantContext {
    TenantContext::new(org_id, user_id).expect("tenant should be valid")
}

fn engine_with_mocks(
    fact_store: Arc<dyn FactStore>,
    vector_store: Arc<dyn VectorStore>,
    llm: MockLlmProvider,
    embedding: MockEmbeddingProvider,
) -> MemoryEngine {
    MemoryEngine::new(fact_store, vector_store, Arc::new(llm), Arc::new(embedding))
}

fn default_engine() -> MemoryEngine {
    engine_with_mocks(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        MockLlmProvider::new(),
        MockEmbeddingProvider::new(4),
    )
}

#[tokio::test]
async fn add_memory_inserts_extracted_facts() {
    let fact_store = Arc::new(MockFactStore::new());
    let engine = engine_with_mocks(
        fact_store.clone(),
        Arc::new(MockVectorStore::new()),
        MockLlmProvider::new(),
        MockEmbeddingProvider::new(4),
    );

    let tenant = tenant("org_a", "user_a");
    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "I am learning Rust.".to_string(),
            }],
            metadata: json!({ "session": "abc" }),
        })
        .await
        .expect("add memory should succeed");

    assert_eq!(output.added, 1);
    assert_eq!(output.memories.len(), 1);

    let stored = fact_store
        .get_fact(&tenant, output.memories[0].id)
        .await
        .expect("get should succeed")
        .expect("fact should exist");
    assert_eq!(stored.content, "I am learning Rust.");
}

#[tokio::test]
async fn add_memory_creates_vectors_for_inserted_facts() {
    let vector_store = Arc::new(MockVectorStore::new());
    let engine = engine_with_mocks(
        Arc::new(MockFactStore::new()),
        vector_store.clone(),
        MockLlmProvider::new(),
        MockEmbeddingProvider::new(4),
    );

    let tenant = tenant("org_a", "user_a");
    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "Vector content".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory should succeed");

    let results = vector_store
        .search_vectors(VectorSearchQuery {
            tenant,
            embedding: vec![1.0, 0.0, 0.0, 0.0],
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("vector search should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].fact_id, output.memories[0].id);
}

#[tokio::test]
async fn embedding_provider_output_is_stored_in_vector_record() {
    let vector_store = Arc::new(MockVectorStore::new());
    let content = "I am learning Rust.";
    let engine = engine_with_mocks(
        Arc::new(MockFactStore::new()),
        vector_store.clone(),
        MockLlmProvider::new(),
        MockEmbeddingProvider::new(4),
    );

    let tenant = tenant("org_a", "user_a");
    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: content.to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory should succeed");

    let expected =
        deterministic_embedding(content, 4).expect("deterministic embedding should succeed");
    let results = vector_store
        .search_vectors(VectorSearchQuery {
            tenant: tenant.clone(),
            embedding: expected.clone(),
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("vector search should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].fact_id, output.memories[0].id);
    assert!(
        results[0].score > 0.99,
        "stored vector should match embedding provider output (score={})",
        results[0].score
    );
    assert_eq!(results[0].content, content);
}

#[tokio::test]
async fn empty_messages_returns_validation_error() {
    let engine = default_engine();
    let error = engine
        .add_memory(AddMemoryInput {
            tenant: tenant("org_a", "user_a"),
            messages: vec![],
            metadata: json!({}),
        })
        .await
        .expect_err("empty messages should fail");

    assert_eq!(
        error,
        MemcoreError::ValidationError("messages cannot be empty".to_string())
    );
}

#[tokio::test]
async fn empty_tenant_org_id_returns_validation_error() {
    let engine = default_engine();
    let error = engine
        .add_memory(AddMemoryInput {
            tenant: TenantContext {
                org_id: "   ".to_string(),
                user_id: "user_a".to_string(),
            },
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect_err("empty org_id should fail");

    assert_eq!(
        error,
        MemcoreError::ValidationError("org_id cannot be empty".to_string())
    );
}

#[tokio::test]
async fn empty_tenant_user_id_returns_validation_error() {
    let engine = default_engine();
    let error = engine
        .add_memory(AddMemoryInput {
            tenant: TenantContext {
                org_id: "org_a".to_string(),
                user_id: "".to_string(),
            },
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect_err("empty user_id should fail");

    assert_eq!(
        error,
        MemcoreError::ValidationError("user_id cannot be empty".to_string())
    );
}

#[tokio::test]
async fn candidate_below_importance_threshold_is_skipped() {
    let low_importance = CandidateFact::new(
        "low value",
        MemoryType::Conversation,
        0.9,
        0.2,
        None,
        json!({}),
    )
    .expect("candidate should be valid");

    let engine = engine_with_mocks(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        MockLlmProvider::new().with_extraction_candidates(vec![low_importance]),
        MockEmbeddingProvider::new(4),
    )
    .with_min_importance(0.55);

    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant("org_a", "user_a"),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "ignored".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory should succeed");

    assert_eq!(output.added, 0);
    assert_eq!(output.noop, 1);
    assert!(output.memories.is_empty());
}

#[tokio::test]
async fn operation_summary_added_count_is_correct() {
    let custom_a = CandidateFact::new(
        "first",
        MemoryType::Skill,
        0.9,
        0.8,
        None,
        json!({}),
    )
    .expect("candidate should be valid");
    let custom_b = CandidateFact::new(
        "second",
        MemoryType::Preference,
        0.85,
        0.7,
        None,
        json!({}),
    )
    .expect("candidate should be valid");

    let engine = engine_with_mocks(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        MockLlmProvider::new().with_extraction_candidates(vec![custom_a, custom_b]),
        MockEmbeddingProvider::new(4),
    );

    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant("org_a", "user_a"),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "batch".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory should succeed");

    assert_eq!(output.added, 2);
    assert_eq!(output.updated, 0);
    assert_eq!(output.deleted, 0);
    assert_eq!(output.noop, 0);
}

#[tokio::test]
async fn configurable_llm_extraction_is_used_without_external_calls() {
    let custom = CandidateFact::new(
        "Configured extraction",
        MemoryType::System,
        0.95,
        0.9,
        None,
        json!({ "source": "test" }),
    )
    .expect("candidate should be valid");

    let llm = MockLlmProvider::new().with_extraction_candidates(vec![custom]);
    let engine = engine_with_mocks(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        llm,
        MockEmbeddingProvider::new(4),
    );

    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant("org_a", "user_a"),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "trigger".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory should succeed");

    assert_eq!(output.memories[0].content, "Configured extraction");
    assert_eq!(output.memories[0].metadata["source"], "test");
}

#[tokio::test]
async fn search_memory_returns_vector_matches_after_add() {
    let content = "I am learning Rust.";
    let engine = engine_with_mocks(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        MockLlmProvider::new(),
        MockEmbeddingProvider::new(4),
    );

    let tenant = tenant("org_a", "user_a");
    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: content.to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory should succeed");

    let output = engine
        .search_memory(SearchMemoryInput {
            tenant,
            query: content.to_string(),
            limit: 10,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search memory should succeed");

    assert_eq!(output.results.len(), 1);
    assert_eq!(output.results[0].content, content);
    assert!(output.results[0].score > 0.99);
}

#[tokio::test]
async fn build_context_formats_memories_after_add() {
    let content = "I am learning Rust.";
    let engine = engine_with_mocks(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        MockLlmProvider::new(),
        MockEmbeddingProvider::new(4),
    );

    let tenant = tenant("org_a", "user_a");
    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: content.to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add memory should succeed");

    let output = engine
        .build_context(BuildContextInput {
            tenant,
            query: content.to_string(),
            max_memories: 10,
            memory_types: None,
            include_metadata: false,
        })
        .await
        .expect("build context should succeed");

    assert!(output.context.contains("Relevant long-term memories:"));
    assert!(output.context.contains(content));
    assert_eq!(output.memories.len(), 1);
}

#[tokio::test]
async fn build_context_returns_empty_message_when_no_matches() {
    let engine = default_engine();
    let tenant = tenant("org_a", "user_a");

    let output = engine
        .build_context(BuildContextInput {
            tenant,
            query: "nothing stored yet".to_string(),
            max_memories: 10,
            memory_types: None,
            include_metadata: false,
        })
        .await
        .expect("build context should succeed");

    assert_eq!(output.context, EMPTY_CONTEXT_MESSAGE);
    assert!(output.memories.is_empty());
}
