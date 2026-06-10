//! Qdrant VectorStore integration tests (skipped without MEMCORE_TEST_QDRANT_URL).

use memcore_core::{MemoryType, TenantContext};
use memcore_storage::{QdrantVectorStore, VectorRecord, VectorSearchQuery, VectorStore};
use serde_json::json;
use uuid::Uuid;

const DIMENSIONS: usize = 4;

fn qdrant_url() -> Option<String> {
    match std::env::var("MEMCORE_TEST_QDRANT_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => None,
    }
}

fn tenant(org_id: &str, user_id: &str) -> TenantContext {
    TenantContext::new(org_id, user_id).expect("tenant should be valid")
}

fn embedding(values: [f32; 4]) -> Vec<f32> {
    values.to_vec()
}

fn sample_record(
    org_id: &str,
    user_id: &str,
    fact_id: Uuid,
    content: &str,
    memory_type: MemoryType,
    metadata: serde_json::Value,
    values: [f32; 4],
) -> VectorRecord {
    VectorRecord {
        id: Uuid::new_v4(),
        fact_id,
        org_id: org_id.to_string(),
        user_id: user_id.to_string(),
        embedding: embedding(values),
        content: content.to_string(),
        memory_type,
        metadata,
    }
}

async fn with_qdrant_store<F, Fut>(test_name: &str, test: F)
where
    F: FnOnce(QdrantVectorStore) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let Some(url) = qdrant_url() else {
        eprintln!("skipping qdrant test `{test_name}`: MEMCORE_TEST_QDRANT_URL not set");
        return;
    };

    let collection = format!("memcore_vectors_test_{}", Uuid::new_v4().simple());
    let store = QdrantVectorStore::connect(&url, &collection, DIMENSIONS)
        .await
        .expect("qdrant store should connect");
    test(store).await;
}

#[tokio::test]
async fn qdrant_connect_creates_collection() {
    with_qdrant_store("qdrant_connect_creates_collection", |_store| async move {
        // connect() in with_qdrant_store already verifies collection creation.
    })
    .await;
}

#[tokio::test]
async fn qdrant_upsert_and_search_vector() {
    with_qdrant_store("qdrant_upsert_and_search_vector", |store| async move {
        let tenant = tenant("org_qdrant_a", "user_a");
        let fact_id = Uuid::new_v4();

        store
            .upsert_vector(
                &tenant,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    fact_id,
                    "learning rust",
                    MemoryType::Skill,
                    json!({}),
                    [0.1, 0.2, 0.3, 0.4],
                ),
            )
            .await
            .expect("upsert should succeed");

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant: tenant.clone(),
                embedding: embedding([0.1, 0.2, 0.3, 0.4]),
                limit: 5,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_id, fact_id);
        assert_eq!(results[0].content, "learning rust");
        assert!(results[0].score > 0.0);
    })
    .await;
}

#[tokio::test]
async fn qdrant_search_respects_org_isolation() {
    with_qdrant_store("qdrant_search_respects_org_isolation", |store| async move {
        let tenant_a = tenant("org_qdrant_a", "user_a");
        let tenant_b = tenant("org_qdrant_b", "user_a");

        store
            .upsert_vector(
                &tenant_a,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    Uuid::new_v4(),
                    "org a only",
                    MemoryType::Skill,
                    json!({}),
                    [1.0, 0.0, 0.0, 0.0],
                ),
            )
            .await
            .expect("upsert should succeed");

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant: tenant_b,
                embedding: embedding([1.0, 0.0, 0.0, 0.0]),
                limit: 5,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert!(results.is_empty());
    })
    .await;
}

#[tokio::test]
async fn qdrant_search_respects_user_isolation() {
    with_qdrant_store("qdrant_search_respects_user_isolation", |store| async move {
        let tenant_a = tenant("org_qdrant_a", "user_a");
        let tenant_b = tenant("org_qdrant_a", "user_b");

        store
            .upsert_vector(
                &tenant_a,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    Uuid::new_v4(),
                    "user a only",
                    MemoryType::Skill,
                    json!({}),
                    [0.0, 1.0, 0.0, 0.0],
                ),
            )
            .await
            .expect("upsert should succeed");

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant: tenant_b,
                embedding: embedding([0.0, 1.0, 0.0, 0.0]),
                limit: 5,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert!(results.is_empty());
    })
    .await;
}

#[tokio::test]
async fn qdrant_upsert_same_fact_id_replaces_record() {
    with_qdrant_store("qdrant_upsert_same_fact_id_replaces_record", |store| async move {
        let tenant = tenant("org_qdrant_a", "user_a");
        let fact_id = Uuid::new_v4();

        store
            .upsert_vector(
                &tenant,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    fact_id,
                    "first version",
                    MemoryType::Skill,
                    json!({}),
                    [0.1, 0.1, 0.1, 0.1],
                ),
            )
            .await
            .expect("first upsert should succeed");

        store
            .upsert_vector(
                &tenant,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    fact_id,
                    "second version",
                    MemoryType::Project,
                    json!({"v": 2}),
                    [0.9, 0.9, 0.9, 0.9],
                ),
            )
            .await
            .expect("second upsert should succeed");

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant,
                embedding: embedding([0.9, 0.9, 0.9, 0.9]),
                limit: 10,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_id, fact_id);
        assert_eq!(results[0].content, "second version");
        assert_eq!(results[0].memory_type, MemoryType::Project);
    })
    .await;
}

#[tokio::test]
async fn qdrant_delete_by_fact_id_removes_tenant_scoped_record() {
    with_qdrant_store("qdrant_delete_by_fact_id_removes_tenant_scoped_record", |store| async move {
        let tenant = tenant("org_qdrant_a", "user_a");
        let fact_id = Uuid::new_v4();

        store
            .upsert_vector(
                &tenant,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    fact_id,
                    "to delete",
                    MemoryType::Skill,
                    json!({}),
                    [0.2, 0.2, 0.2, 0.2],
                ),
            )
            .await
            .expect("upsert should succeed");

        store
            .delete_by_fact_id(&tenant, fact_id)
            .await
            .expect("delete should succeed");

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant,
                embedding: embedding([0.2, 0.2, 0.2, 0.2]),
                limit: 5,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert!(results.is_empty());
    })
    .await;
}

#[tokio::test]
async fn qdrant_delete_by_user_removes_only_that_user_vectors() {
    with_qdrant_store("qdrant_delete_by_user_removes_only_that_user_vectors", |store| async move {
        let tenant_a = tenant("org_qdrant_a", "user_a");
        let tenant_b = tenant("org_qdrant_a", "user_b");

        store
            .upsert_vector(
                &tenant_a,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    Uuid::new_v4(),
                    "user a",
                    MemoryType::Skill,
                    json!({}),
                    [0.3, 0.3, 0.3, 0.3],
                ),
            )
            .await
            .expect("upsert a should succeed");

        store
            .upsert_vector(
                &tenant_b,
                sample_record(
                    "org_qdrant_a",
                    "user_b",
                    Uuid::new_v4(),
                    "user b",
                    MemoryType::Skill,
                    json!({}),
                    [0.4, 0.4, 0.4, 0.4],
                ),
            )
            .await
            .expect("upsert b should succeed");

        store
            .delete_by_user(&tenant_a)
            .await
            .expect("delete user should succeed");

        let results_a = store
            .search_vectors(VectorSearchQuery {
                tenant: tenant_a,
                embedding: embedding([0.3, 0.3, 0.3, 0.3]),
                limit: 5,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search a should succeed");
        assert!(results_a.is_empty());

        let results_b = store
            .search_vectors(VectorSearchQuery {
                tenant: tenant_b,
                embedding: embedding([0.4, 0.4, 0.4, 0.4]),
                limit: 5,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search b should succeed");
        assert_eq!(results_b.len(), 1);
    })
    .await;
}

#[tokio::test]
async fn qdrant_memory_type_round_trip() {
    with_qdrant_store("qdrant_memory_type_round_trip", |store| async move {
        let tenant = tenant("org_qdrant_a", "user_a");
        let fact_id = Uuid::new_v4();

        store
            .upsert_vector(
                &tenant,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    fact_id,
                    "preference memory",
                    MemoryType::Preference,
                    json!({}),
                    [0.5, 0.5, 0.5, 0.5],
                ),
            )
            .await
            .expect("upsert should succeed");

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant,
                embedding: embedding([0.5, 0.5, 0.5, 0.5]),
                limit: 1,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results[0].memory_type, MemoryType::Preference);
    })
    .await;
}

#[tokio::test]
async fn qdrant_metadata_round_trip() {
    with_qdrant_store("qdrant_metadata_round_trip", |store| async move {
        let tenant = tenant("org_qdrant_a", "user_a");
        let fact_id = Uuid::new_v4();
        let metadata = json!({ "topic": "rust", "level": 3 });

        store
            .upsert_vector(
                &tenant,
                sample_record(
                    "org_qdrant_a",
                    "user_a",
                    fact_id,
                    "metadata test",
                    MemoryType::Skill,
                    metadata.clone(),
                    [0.6, 0.6, 0.6, 0.6],
                ),
            )
            .await
            .expect("upsert should succeed");

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant,
                embedding: embedding([0.6, 0.6, 0.6, 0.6]),
                limit: 1,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results[0].metadata, metadata);
    })
    .await;
}

#[tokio::test]
async fn qdrant_search_respects_limit() {
    with_qdrant_store("qdrant_search_respects_limit", |store| async move {
        let tenant = tenant("org_qdrant_a", "user_a");

        for i in 0..5 {
            let v = (i as f32) * 0.1;
            store
                .upsert_vector(
                    &tenant,
                    sample_record(
                        "org_qdrant_a",
                        "user_a",
                        Uuid::new_v4(),
                        &format!("memory {i}"),
                        MemoryType::Skill,
                        json!({}),
                        [v, v, v, v],
                    ),
                )
                .await
                .expect("upsert should succeed");
        }

        let results = store
            .search_vectors(VectorSearchQuery {
                tenant,
                embedding: embedding([0.2, 0.2, 0.2, 0.2]),
                limit: 2,
                memory_types: None,
                metadata_filter: None,
            })
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 2);
    })
    .await;
}
