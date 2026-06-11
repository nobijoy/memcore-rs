use std::sync::Arc;

use async_trait::async_trait;
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{
    AddMemoryInput, CandidateFact, DeleteMemoryInput, FactOperation, FactOperationDecision,
    ForgetUserInput, MemoryEngine, MemoryEventOperation, MemoryEventQuery, MemoryEventStore,
    MemoryMessage, MemoryType, MessageRole, TenantContext,
};
use memcore_providers::{MockEmbeddingProvider, MockLlmProvider};
use memcore_storage::{MockFactStore, MockMemoryEventStore, MockVectorStore};
use serde_json::json;

fn tenant(org_id: &str, user_id: &str) -> TenantContext {
    TenantContext::new(org_id, user_id).expect("tenant should be valid")
}

fn high_importance_candidate(content: &str) -> CandidateFact {
    CandidateFact::new(content, MemoryType::Preference, 0.9, 0.8, None, json!({}))
        .expect("candidate should be valid")
}

fn engine_with_audit(
    fact_store: Arc<MockFactStore>,
    vector_store: Arc<MockVectorStore>,
    event_store: Arc<MockMemoryEventStore>,
    llm: MockLlmProvider,
) -> MemoryEngine {
    MemoryEngine::new(
        fact_store,
        vector_store,
        Arc::new(llm),
        Arc::new(MockEmbeddingProvider::new(4)),
    )
    .with_event_store(event_store)
    .with_audit_provider_info(Some("mock".to_string()), Some("mock-llm".to_string()))
}

struct FailingMemoryEventStore;

#[async_trait]
impl MemoryEventStore for FailingMemoryEventStore {
    async fn record_event(
        &self,
        _tenant: &TenantContext,
        _event: memcore_core::MemoryEvent,
    ) -> MemcoreResult<memcore_core::MemoryEvent> {
        Err(MemcoreError::StorageError(
            "audit store unavailable".to_string(),
        ))
    }

    async fn list_events(
        &self,
        _query: MemoryEventQuery,
    ) -> MemcoreResult<Vec<memcore_core::MemoryEvent>> {
        Err(MemcoreError::StorageError(
            "audit store unavailable".to_string(),
        ))
    }

    async fn delete_events_older_than(
        &self,
        _tenant: &TenantContext,
        _cutoff: chrono::DateTime<chrono::Utc>,
        _dry_run: bool,
    ) -> MemcoreResult<usize> {
        Err(MemcoreError::StorageError(
            "audit store unavailable".to_string(),
        ))
    }
}

#[tokio::test]
async fn add_operation_records_add_event() {
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
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

    let events = event_store
        .list_events(MemoryEventQuery::new(tenant, 10))
        .await
        .expect("list events should succeed");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, MemoryEventOperation::Add);
    assert_eq!(events[0].fact_id, Some(output.memories[0].id));
    assert_eq!(events[0].new_content.as_deref(), Some("I am learning Rust."));
    assert!(events[0].input_text.is_none());
    assert_eq!(events[0].provider_name.as_deref(), Some("mock"));
}

#[tokio::test]
async fn update_operation_records_update_event() {
    let fact_store = Arc::new(MockFactStore::new());
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        fact_store.clone(),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
    );

    let tenant = tenant("org_a", "user_a");
    let initial = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "User prefers Python.".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("initial add should succeed");

    let target_id = initial.memories[0].id;
    let engine = engine_with_audit(
        fact_store,
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new()
            .with_extraction_candidates(vec![high_importance_candidate("User prefers Rust.")])
            .with_classification_decision(FactOperationDecision {
                operation: FactOperation::Update,
                target_fact_id: Some(target_id),
                reason: Some("preference changed".to_string()),
                confidence: 0.95,
            }),
    );

    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "User prefers Rust.".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("update should succeed");

    let events = event_store
        .list_events(MemoryEventQuery::new(tenant, 10))
        .await
        .expect("list events should succeed");

    let update_event = events
        .iter()
        .find(|event| event.operation == MemoryEventOperation::Update)
        .expect("update event should exist");
    assert_eq!(update_event.fact_id, Some(target_id));
    assert_eq!(
        update_event.previous_content.as_deref(),
        Some("User prefers Python.")
    );
    assert_eq!(
        update_event.new_content.as_deref(),
        Some("User prefers Rust.")
    );
}

#[tokio::test]
async fn delete_operation_records_delete_event() {
    let fact_store = Arc::new(MockFactStore::new());
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        fact_store.clone(),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
    );

    let tenant = tenant("org_a", "user_a");
    let initial = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "remove this".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("initial add should succeed");

    let target_id = initial.memories[0].id;
    let engine = engine_with_audit(
        fact_store,
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new()
            .with_extraction_candidates(vec![high_importance_candidate("remove this")])
            .with_classification_decision(FactOperationDecision {
                operation: FactOperation::Delete,
                target_fact_id: Some(target_id),
                reason: None,
                confidence: 0.9,
            }),
    );

    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "remove this".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("delete lifecycle should succeed");

    let events = event_store
        .list_events(MemoryEventQuery::new(tenant, 10))
        .await
        .expect("list events should succeed");

    let delete_event = events
        .iter()
        .find(|event| event.operation == MemoryEventOperation::Delete)
        .expect("delete event should exist");
    assert_eq!(delete_event.fact_id, Some(target_id));
    assert_eq!(delete_event.previous_content.as_deref(), Some("remove this"));
}

#[tokio::test]
async fn noop_operation_records_noop_event() {
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new()
            .with_extraction_candidates(vec![high_importance_candidate("skip me")])
            .with_classification_decision(FactOperationDecision {
                operation: FactOperation::NoOp,
                target_fact_id: None,
                reason: Some("duplicate".to_string()),
                confidence: 0.9,
            }),
    );

    let tenant = tenant("org_a", "user_a");
    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "skip me".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("noop should succeed");

    let events = event_store
        .list_events(MemoryEventQuery::new(tenant, 10))
        .await
        .expect("list events should succeed");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, MemoryEventOperation::NoOp);
    assert_eq!(events[0].metadata["reason"], "duplicate");
}

#[tokio::test]
async fn forget_user_records_forget_user_event() {
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
    );

    let tenant = tenant("org_a", "user_a");
    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "temporary".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    engine
        .forget_user(ForgetUserInput {
            tenant: tenant.clone(),
        })
        .await
        .expect("forget should succeed");

    let events = event_store
        .list_events(MemoryEventQuery::new(tenant, 10))
        .await
        .expect("list events should succeed");

    let forget_event = events
        .iter()
        .find(|event| event.operation == MemoryEventOperation::ForgetUser)
        .expect("forget event should exist");
    assert!(forget_event.fact_id.is_none());
    assert_eq!(forget_event.metadata["deleted"], true);
}

#[tokio::test]
async fn event_store_enforces_tenant_isolation_on_list() {
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
    );

    let tenant_a = tenant("org_a", "user_a");
    let tenant_b = tenant("org_a", "user_b");

    engine
        .add_memory(AddMemoryInput {
            tenant: tenant_a.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "only user a".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    let listed_a = event_store
        .list_events(MemoryEventQuery::new(tenant_a, 10))
        .await
        .expect("list should succeed");
    assert_eq!(listed_a.len(), 1);

    let listed_b = event_store
        .list_events(MemoryEventQuery::new(tenant_b, 10))
        .await
        .expect("list should succeed");
    assert!(listed_b.is_empty());
}

#[tokio::test]
async fn listing_events_by_fact_id_works() {
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
    );

    let tenant = tenant("org_a", "user_a");
    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "target fact".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    let fact_id = output.memories[0].id;
    let mut query = MemoryEventQuery::new(tenant.clone(), 10);
    query.fact_id = Some(fact_id);

    let events = event_store
        .list_events(query)
        .await
        .expect("list should succeed");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].fact_id, Some(fact_id));
}

#[tokio::test]
async fn listing_events_by_operation_works() {
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
    );

    let tenant = tenant("org_a", "user_a");
    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "hello".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    let mut query = MemoryEventQuery::new(tenant, 10);
    query.operation = Some(MemoryEventOperation::Add);

    let events = event_store
        .list_events(query)
        .await
        .expect("list should succeed");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, MemoryEventOperation::Add);
}

#[tokio::test]
async fn audit_event_failure_does_not_break_main_operation() {
    let engine = MemoryEngine::new(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        Arc::new(MockLlmProvider::new()),
        Arc::new(MockEmbeddingProvider::new(4)),
    )
    .with_event_store(Arc::new(FailingMemoryEventStore));

    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant("org_a", "user_a"),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "still works".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("main operation should succeed despite audit failure");

    assert_eq!(output.added, 1);
}

#[tokio::test]
async fn delete_memory_route_records_delete_event() {
    let event_store = Arc::new(MockMemoryEventStore::new());
    let engine = engine_with_audit(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        event_store.clone(),
        MockLlmProvider::new(),
    );

    let tenant = tenant("org_a", "user_a");
    let output = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "delete via route".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    let memory_id = output.memories[0].id;
    engine
        .delete_memory(DeleteMemoryInput {
            tenant: tenant.clone(),
            memory_id,
        })
        .await
        .expect("delete should succeed");

    let mut query = MemoryEventQuery::new(tenant, 10);
    query.operation = Some(MemoryEventOperation::Delete);

    let events = event_store
        .list_events(query)
        .await
        .expect("list should succeed");
    assert!(
        events
            .iter()
            .any(|event| event.metadata.get("source") == Some(&json!("delete_memory")))
    );
}
