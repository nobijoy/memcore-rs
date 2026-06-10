use std::sync::Arc;

use chrono::Utc;
use memcore_core::{
    AddMemoryInput, ExportUserDataInput, ImportMode, ImportUserDataInput, ListMemoriesInput,
    MemoryEngine, MemoryMessage, MemorySource, MemoryType, MessageRole, SearchMemoryInput,
    TenantContext, USER_EXPORT_FORMAT_VERSION, UserMemoryExport,
};
use memcore_providers::{MockEmbeddingProvider, MockLlmProvider};
use memcore_storage::{MockFactStore, MockMemoryEventStore, MockVectorStore};
use serde_json::json;
use uuid::Uuid;

fn tenant(org_id: &str, user_id: &str) -> TenantContext {
    TenantContext::new(org_id, user_id).expect("tenant should be valid")
}

fn engine_with_events() -> MemoryEngine {
    MemoryEngine::new(
        Arc::new(MockFactStore::new()),
        Arc::new(MockVectorStore::new()),
        Arc::new(MockLlmProvider::new()),
        Arc::new(MockEmbeddingProvider::new(4)),
    )
    .with_event_store(Arc::new(MockMemoryEventStore::new()))
}

async fn seed_export(engine: &MemoryEngine, org_id: &str, user_id: &str) -> UserMemoryExport {
    let tenant = tenant(org_id, user_id);
    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "Import source memory".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    engine
        .export_user_data(ExportUserDataInput {
            tenant,
            include_events: true,
            include_deleted: false,
        })
        .await
        .expect("export should succeed")
}

fn manual_fact(org_id: &str, user_id: &str, content: &str) -> memcore_core::Fact {
    memcore_core::Fact::new(
        Uuid::new_v4(),
        org_id,
        user_id,
        MemoryType::Profile,
        content,
        None,
        MemorySource::ApiImport,
        0.9,
        0.8,
        None,
        None,
        Utc::now(),
        Utc::now(),
        json!({}),
    )
    .expect("fact")
}

#[tokio::test]
async fn import_append_adds_exported_facts() {
    let source = engine_with_events();
    let export = seed_export(&source, "org_a", "user_a").await;

    let target = engine_with_events();
    let tenant = tenant("org_a", "user_a");

    let output = target
        .import_user_data(ImportUserDataInput {
            tenant,
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect("import should succeed");

    assert_eq!(output.imported_facts, 1);
    assert_eq!(output.imported_events, 0);
    assert!(!output.replaced_existing);
}

#[tokio::test]
async fn import_replace_removes_previous_user_facts() {
    let engine = engine_with_events();
    let tenant = tenant("org_a", "user_a");

    engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "old memory".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    let mut export = UserMemoryExport::new("org_a", "user_a", vec![], vec![]);
    export.facts.push(manual_fact("org_a", "user_a", "replacement"));

    let output = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant.clone(),
            export,
            mode: ImportMode::Replace,
            restore_events: false,
        })
        .await
        .expect("import should succeed");

    assert!(output.replaced_existing);
    assert_eq!(output.imported_facts, 1);

    let listed = engine
        .list_memories(ListMemoriesInput {
            tenant,
            memory_type: None,
            limit: 20,
            cursor: None,
            include_deleted: false,
        })
        .await
        .expect("list should succeed");

    assert_eq!(listed.memories.len(), 1);
    assert_eq!(listed.memories[0].content, "replacement");
}

#[tokio::test]
async fn import_regenerates_vectors_and_enables_search() {
    let source = engine_with_events();
    let export = seed_export(&source, "org_a", "user_a").await;

    let target = engine_with_events();
    let tenant = tenant("org_a", "user_a");

    target
        .import_user_data(ImportUserDataInput {
            tenant: tenant.clone(),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect("import should succeed");

    let search = target
        .search_memory(SearchMemoryInput {
            tenant,
            query: "Import source memory".to_string(),
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        })
        .await
        .expect("search should succeed");

    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].content, "Import source memory");
}

#[tokio::test]
async fn import_rejects_mismatched_org_id() {
    let engine = engine_with_events();
    let mut export = UserMemoryExport::new("org_b", "user_a", vec![], vec![]);
    export.facts.push(manual_fact("org_b", "user_a", "x"));

    let err = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect_err("should reject org mismatch");

    assert!(matches!(
        err,
        memcore_common::MemcoreError::ValidationError(_)
    ));
}

#[tokio::test]
async fn import_rejects_mismatched_user_id() {
    let engine = engine_with_events();
    let mut export = UserMemoryExport::new("org_a", "user_b", vec![], vec![]);
    export.facts.push(manual_fact("org_a", "user_b", "x"));

    let err = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect_err("should reject user mismatch");

    assert!(matches!(
        err,
        memcore_common::MemcoreError::ValidationError(_)
    ));
}

#[tokio::test]
async fn import_rejects_mismatched_fact_tenant() {
    let engine = engine_with_events();
    let mut export = UserMemoryExport::new("org_a", "user_a", vec![], vec![]);
    export.facts.push(manual_fact("org_a", "user_b", "x"));

    let err = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect_err("should reject fact tenant mismatch");

    assert!(matches!(
        err,
        memcore_common::MemcoreError::ValidationError(_)
    ));
}

#[tokio::test]
async fn import_rejects_unsupported_format_version() {
    let engine = engine_with_events();
    let mut export = UserMemoryExport::new("org_a", "user_a", vec![], vec![]);
    export.format_version = "memcore.user_export.v0".to_string();

    let err = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect_err("should reject format version");

    assert!(matches!(
        err,
        memcore_common::MemcoreError::ValidationError(_)
    ));
}

#[tokio::test]
async fn import_rejects_invalid_confidence() {
    let engine = engine_with_events();
    let mut fact = manual_fact("org_a", "user_a", "bad confidence");
    fact.confidence = 1.5;
    let export = UserMemoryExport::new("org_a", "user_a", vec![fact], vec![]);

    let err = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect_err("should reject confidence");

    assert!(matches!(
        err,
        memcore_common::MemcoreError::ValidationError(_)
    ));
}

#[tokio::test]
async fn import_rejects_empty_fact_content() {
    let engine = engine_with_events();
    let fact: memcore_core::Fact = serde_json::from_value(json!({
        "id": Uuid::new_v4(),
        "org_id": "org_a",
        "user_id": "user_a",
        "content": "   ",
        "summary": null,
        "memory_type": "profile",
        "source": "api_import",
        "confidence": 0.9,
        "importance": 0.8,
        "valid_at": null,
        "invalid_at": null,
        "recorded_at": Utc::now(),
        "updated_at": Utc::now(),
        "metadata": {}
    }))
    .expect("fact json");
    let export = UserMemoryExport::new("org_a", "user_a", vec![fact], vec![]);

    let err = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect_err("should reject empty content");

    assert!(matches!(
        err,
        memcore_common::MemcoreError::ValidationError(_)
    ));
}

#[tokio::test]
async fn import_rejects_secret_metadata() {
    let engine = engine_with_events();
    let mut fact = manual_fact("org_a", "user_a", "secret metadata");
    fact.metadata = json!({ "api_key": "mc_live_secret" });
    let export = UserMemoryExport::new("org_a", "user_a", vec![fact], vec![]);

    let err = engine
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect_err("should reject secrets");

    assert!(matches!(
        err,
        memcore_common::MemcoreError::ValidationError(_)
    ));
}

#[tokio::test]
async fn restore_events_false_skips_event_import() {
    let source = engine_with_events();
    let export = seed_export(&source, "org_a", "user_a").await;
    assert!(!export.memory_events.is_empty());

    let target = engine_with_events();
    let output = target
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect("import should succeed");

    assert_eq!(output.imported_events, 0);
}

#[tokio::test]
async fn restore_events_true_imports_events_when_supported() {
    let source = engine_with_events();
    let export = seed_export(&source, "org_a", "user_a").await;

    let target = engine_with_events();
    let output = target
        .import_user_data(ImportUserDataInput {
            tenant: tenant("org_a", "user_a"),
            export,
            mode: ImportMode::Append,
            restore_events: true,
        })
        .await
        .expect("import should succeed");

    assert!(output.imported_events > 0);
}

#[tokio::test]
async fn append_mode_regenerates_id_on_collision() {
    let engine = engine_with_events();
    let tenant = tenant("org_a", "user_a");

    let added = engine
        .add_memory(AddMemoryInput {
            tenant: tenant.clone(),
            messages: vec![MemoryMessage {
                role: MessageRole::User,
                content: "existing".to_string(),
            }],
            metadata: json!({}),
        })
        .await
        .expect("add should succeed");

    let existing_id = added.memories[0].id;
    let mut export = UserMemoryExport::new("org_a", "user_a", vec![], vec![]);
    export
        .facts
        .push(manual_fact("org_a", "user_a", "imported duplicate id"));
    export.facts[0].id = existing_id;

    let output = engine
        .import_user_data(ImportUserDataInput {
            tenant,
            export,
            mode: ImportMode::Append,
            restore_events: false,
        })
        .await
        .expect("import should succeed with new id");

    assert_eq!(output.imported_facts, 1);
}

#[test]
fn export_format_version_constant_matches_import_expectation() {
    assert_eq!(USER_EXPORT_FORMAT_VERSION, "memcore.user_export.v1");
}
