//! Postgres ApiKeyStore integration tests (skipped without MEMCORE_TEST_POSTGRES_URL).

use chrono::Utc;
use memcore_common::hash_api_key;
use memcore_core::{ApiKeyRecord, ApiKeyScope};
use memcore_storage::{ApiKeyStore, PostgresApiKeyStore};
use uuid::Uuid;

fn postgres_url() -> Option<String> {
    match std::env::var("MEMCORE_TEST_POSTGRES_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => None,
    }
}

async fn with_postgres_store<F, Fut>(test_name: &str, test: F)
where
    F: FnOnce(PostgresApiKeyStore) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let Some(url) = postgres_url() else {
        eprintln!("skipping postgres test `{test_name}`: MEMCORE_TEST_POSTGRES_URL not set");
        return;
    };
    let store = PostgresApiKeyStore::connect(&url)
        .await
        .expect("postgres api key store should connect");
    test(store).await;
}

#[tokio::test]
async fn postgres_insert_find_and_revoke_api_key() {
    with_postgres_store("postgres_insert_find_and_revoke_api_key", |store| async move {
        let record = ApiKeyRecord {
            id: Uuid::new_v4(),
            org_id: "org_pg_key".to_string(),
            name: "pg-test".to_string(),
            key_hash: hash_api_key("pepper", "pg-secret"),
            scopes: vec![ApiKeyScope::MemoryRead, ApiKeyScope::AuditRead],
            created_at: Utc::now(),
            revoked_at: None,
        };

        store
            .insert_api_key(record.clone())
            .await
            .expect("insert should succeed");

        let found = store
            .find_by_hash(&record.key_hash)
            .await
            .expect("find should succeed")
            .expect("record should exist");
        assert_eq!(found.scopes, record.scopes);

        store
            .revoke_api_key("org_pg_key", record.id)
            .await
            .expect("revoke should succeed");
        assert!(store
            .find_by_hash(&record.key_hash)
            .await
            .expect("find should succeed")
            .is_none());
    })
    .await;
}
