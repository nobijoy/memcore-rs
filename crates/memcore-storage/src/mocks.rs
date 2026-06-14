use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{Fact, TenantContext};
use serde_json::Value;
use uuid::Uuid;

use memcore_core::pagination::{is_after_cursor_in_desc_order, page_fetch_limit};
use memcore_core::ports::{
    ApiKeyListQuery, ApiKeyStore, FactSearchQuery, FactStore, MemoryEventQuery, MemoryEventStore,
    OrgMemoryEventQuery, OrgUserListQuery, OrgUserSummary, RetentionPruneResult, VectorRecord,
    VectorSearchQuery, VectorSearchResult, VectorStore,
};
use memcore_core::{ApiKeyRecord, MemoryEvent};

fn event_matches_tenant(event: &MemoryEvent, tenant: &TenantContext) -> bool {
    event.org_id == tenant.org_id && event.user_id == tenant.user_id
}

fn ensure_event_tenant(event: &MemoryEvent, tenant: &TenantContext) -> MemcoreResult<()> {
    if event_matches_tenant(event, tenant) {
        Ok(())
    } else {
        Err(MemcoreError::Forbidden)
    }
}

fn normalize_event_list_limit(limit: usize) -> MemcoreResult<usize> {
    use memcore_core::ports::{DEFAULT_MEMORY_EVENT_LIST_LIMIT, MAX_MEMORY_EVENT_LIST_LIMIT};

    if limit == 0 {
        return Ok(DEFAULT_MEMORY_EVENT_LIST_LIMIT);
    }

    if limit > MAX_MEMORY_EVENT_LIST_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_MEMORY_EVENT_LIST_LIMIT}"
        )));
    }

    Ok(limit)
}

fn tenant_key(tenant: &TenantContext) -> (String, String) {
    (tenant.org_id.clone(), tenant.user_id.clone())
}

fn fact_matches_tenant(fact: &Fact, tenant: &TenantContext) -> bool {
    fact.org_id == tenant.org_id && fact.user_id == tenant.user_id
}

fn record_matches_tenant(record: &VectorRecord, tenant: &TenantContext) -> bool {
    record.org_id == tenant.org_id && record.user_id == tenant.user_id
}

fn ensure_fact_tenant(fact: &Fact, tenant: &TenantContext) -> MemcoreResult<()> {
    if fact_matches_tenant(fact, tenant) {
        Ok(())
    } else {
        Err(MemcoreError::Forbidden)
    }
}

fn ensure_record_tenant(record: &VectorRecord, tenant: &TenantContext) -> MemcoreResult<()> {
    if record_matches_tenant(record, tenant) {
        Ok(())
    } else {
        Err(MemcoreError::Forbidden)
    }
}

fn metadata_matches(record_metadata: &Value, filter: &Value) -> bool {
    let Some(filter_obj) = filter.as_object() else {
        return false;
    };

    let Some(record_obj) = record_metadata.as_object() else {
        return false;
    };

    filter_obj
        .iter()
        .all(|(key, value)| record_obj.get(key) == Some(value))
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[derive(Debug, Default)]
pub struct MockFactStore {
    facts: RwLock<HashMap<Uuid, Fact>>,
    deleted: RwLock<HashSet<Uuid>>,
}

impl MockFactStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn is_deleted(&self, fact_id: &Uuid) -> bool {
        self.deleted
            .read()
            .expect("deleted lock poisoned")
            .contains(fact_id)
    }
}

#[async_trait]
impl FactStore for MockFactStore {
    async fn insert_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact> {
        ensure_fact_tenant(&fact, tenant)?;

        let mut facts = self.facts.write().expect("facts lock poisoned");
        if facts.contains_key(&fact.id) {
            return Err(MemcoreError::Conflict(format!(
                "fact already exists: {}",
                fact.id
            )));
        }

        facts.insert(fact.id, fact.clone());
        Ok(fact)
    }

    async fn update_fact(&self, tenant: &TenantContext, fact: Fact) -> MemcoreResult<Fact> {
        ensure_fact_tenant(&fact, tenant)?;

        let mut facts = self.facts.write().expect("facts lock poisoned");
        let existing = facts
            .get(&fact.id)
            .ok_or_else(|| MemcoreError::NotFound(format!("fact not found: {}", fact.id)))?;

        if !fact_matches_tenant(existing, tenant) {
            return Err(MemcoreError::Forbidden);
        }

        facts.insert(fact.id, fact.clone());
        Ok(fact)
    }

    async fn get_fact(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<Option<Fact>> {
        let facts = self.facts.read().expect("facts lock poisoned");
        let Some(fact) = facts.get(&fact_id) else {
            return Ok(None);
        };

        if !fact_matches_tenant(fact, tenant) {
            return Ok(None);
        }

        if self.is_deleted(&fact_id) {
            return Ok(None);
        }

        Ok(Some(fact.clone()))
    }

    async fn search_facts(&self, query: FactSearchQuery) -> MemcoreResult<Vec<Fact>> {
        let facts = self.facts.read().expect("facts lock poisoned");
        let deleted = self.deleted.read().expect("deleted lock poisoned");

        let mut results: Vec<Fact> = facts
            .values()
            .filter(|fact| fact_matches_tenant(fact, &query.tenant))
            .filter(|fact| {
                if query.include_deleted {
                    true
                } else {
                    !deleted.contains(&fact.id)
                }
            })
            .filter(|fact| {
                query
                    .memory_types
                    .as_ref()
                    .is_none_or(|types| types.contains(&fact.memory_type))
            })
            .filter(|fact| {
                query.query_text.as_ref().is_none_or(|text| {
                    let needle = text.to_ascii_lowercase();
                    fact.content.to_ascii_lowercase().contains(&needle)
                })
            })
            .filter(|fact| {
                query.cursor.as_ref().is_none_or(|cursor| {
                    is_after_cursor_in_desc_order(
                        fact.updated_at,
                        &fact.id.to_string(),
                        cursor,
                    )
                })
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        results.truncate(page_fetch_limit(query.limit));
        Ok(results)
    }

    async fn soft_delete_fact(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<()> {
        let facts = self.facts.read().expect("facts lock poisoned");
        let Some(fact) = facts.get(&fact_id) else {
            return Err(MemcoreError::NotFound(format!("fact not found: {fact_id}")));
        };

        if !fact_matches_tenant(fact, tenant) {
            return Err(MemcoreError::Forbidden);
        }

        self.deleted
            .write()
            .expect("deleted lock poisoned")
            .insert(fact_id);
        Ok(())
    }

    async fn delete_user_data(&self, tenant: &TenantContext) -> MemcoreResult<()> {
        let key = tenant_key(tenant);
        let mut facts = self.facts.write().expect("facts lock poisoned");
        facts.retain(|_, fact| (fact.org_id.as_str(), fact.user_id.as_str()) != (&key.0, &key.1));

        let mut deleted = self.deleted.write().expect("deleted lock poisoned");
        let remaining_ids: HashSet<Uuid> = facts.keys().copied().collect();
        deleted.retain(|id| remaining_ids.contains(id));
        Ok(())
    }

    async fn delete_facts_older_than(
        &self,
        tenant: &TenantContext,
        cutoff: DateTime<Utc>,
        dry_run: bool,
    ) -> MemcoreResult<RetentionPruneResult> {
        let matching_ids: Vec<Uuid> = {
            let facts = self.facts.read().expect("facts lock poisoned");
            let deleted_set = self.deleted.read().expect("deleted lock poisoned");
            facts
                .values()
                .filter(|fact| fact_matches_tenant(fact, tenant))
                .filter(|fact| !deleted_set.contains(&fact.id))
                .filter(|fact| fact.updated_at < cutoff)
                .map(|fact| fact.id)
                .collect()
        };

        if dry_run {
            return Ok(RetentionPruneResult {
                count: matching_ids.len(),
                fact_ids: Vec::new(),
            });
        }

        let mut deleted = self.deleted.write().expect("deleted lock poisoned");
        for fact_id in &matching_ids {
            deleted.insert(*fact_id);
        }

        Ok(RetentionPruneResult {
            count: matching_ids.len(),
            fact_ids: matching_ids,
        })
    }

    async fn count_facts_by_org(&self, org_id: &str) -> MemcoreResult<usize> {
        let facts = self.facts.read().expect("facts lock poisoned");
        let deleted_set = self.deleted.read().expect("deleted lock poisoned");
        Ok(facts
            .values()
            .filter(|fact| fact.org_id == org_id)
            .filter(|fact| !deleted_set.contains(&fact.id))
            .count())
    }

    async fn count_users_by_org(&self, org_id: &str) -> MemcoreResult<usize> {
        let facts = self.facts.read().expect("facts lock poisoned");
        let deleted_set = self.deleted.read().expect("deleted lock poisoned");
        let mut users = HashSet::new();
        for fact in facts.values() {
            if fact.org_id == org_id && !deleted_set.contains(&fact.id) {
                users.insert(fact.user_id.clone());
            }
        }
        Ok(users.len())
    }

    async fn list_users_by_org(
        &self,
        query: OrgUserListQuery,
    ) -> MemcoreResult<Vec<OrgUserSummary>> {
        let facts = self.facts.read().expect("facts lock poisoned");
        let deleted_set = self.deleted.read().expect("deleted lock poisoned");

        let mut aggregates: HashMap<String, (usize, Option<DateTime<Utc>>)> = HashMap::new();
        for fact in facts.values() {
            if fact.org_id != query.org_id || deleted_set.contains(&fact.id) {
                continue;
            }
            let entry = aggregates
                .entry(fact.user_id.clone())
                .or_insert((0, None));
            entry.0 += 1;
            entry.1 = Some(match entry.1 {
                Some(current) => current.max(fact.updated_at),
                None => fact.updated_at,
            });
        }

        let mut users: Vec<OrgUserSummary> = aggregates
            .into_iter()
            .map(|(user_id, (memory_count, last_memory_at))| OrgUserSummary {
                user_id,
                memory_count,
                last_memory_at,
            })
            .filter(|user| {
                query.cursor.as_ref().is_none_or(|cursor| {
                    let sort_value = user.last_memory_at.unwrap_or(DateTime::<Utc>::MIN_UTC);
                    is_after_cursor_in_desc_order(sort_value, &user.user_id, cursor)
                })
            })
            .collect();

        users.sort_by(|a, b| {
            let a_sort = a.last_memory_at.unwrap_or(DateTime::<Utc>::MIN_UTC);
            let b_sort = b.last_memory_at.unwrap_or(DateTime::<Utc>::MIN_UTC);
            b_sort
                .cmp(&a_sort)
                .then_with(|| b.user_id.cmp(&a.user_id))
        });
        users.truncate(page_fetch_limit(query.limit));
        Ok(users)
    }
}

#[derive(Debug, Default)]
pub struct MockVectorStore {
    records: RwLock<HashMap<Uuid, VectorRecord>>,
}

impl MockVectorStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl VectorStore for MockVectorStore {
    async fn upsert_vector(
        &self,
        tenant: &TenantContext,
        record: VectorRecord,
    ) -> MemcoreResult<()> {
        ensure_record_tenant(&record, tenant)?;
        self.records
            .write()
            .expect("records lock poisoned")
            .insert(record.id, record);
        Ok(())
    }

    async fn search_vectors(
        &self,
        query: VectorSearchQuery,
    ) -> MemcoreResult<Vec<VectorSearchResult>> {
        let records = self.records.read().expect("records lock poisoned");

        let mut scored: Vec<VectorSearchResult> = records
            .values()
            .filter(|record| record_matches_tenant(record, &query.tenant))
            .filter(|record| {
                query
                    .memory_types
                    .as_ref()
                    .is_none_or(|types| types.contains(&record.memory_type))
            })
            .filter(|record| {
                query
                    .metadata_filter
                    .as_ref()
                    .is_none_or(|filter| metadata_matches(&record.metadata, filter))
            })
            .map(|record| VectorSearchResult {
                fact_id: record.fact_id,
                content: record.content.clone(),
                score: cosine_similarity(&query.embedding, &record.embedding),
                memory_type: record.memory_type,
                metadata: record.metadata.clone(),
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(query.limit);
        Ok(scored)
    }

    async fn delete_by_fact_id(
        &self,
        tenant: &TenantContext,
        fact_id: Uuid,
    ) -> MemcoreResult<()> {
        let mut records = self.records.write().expect("records lock poisoned");
        let before = records.len();
        records.retain(|_, record| {
            !(record_matches_tenant(record, tenant) && record.fact_id == fact_id)
        });

        if records.len() == before {
            return Err(MemcoreError::NotFound(format!(
                "vector record not found for fact: {fact_id}"
            )));
        }

        Ok(())
    }

    async fn delete_by_user(&self, tenant: &TenantContext) -> MemcoreResult<()> {
        let key = tenant_key(tenant);
        self.records
            .write()
            .expect("records lock poisoned")
            .retain(|_, record| (record.org_id.as_str(), record.user_id.as_str()) != (&key.0, &key.1));
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MockMemoryEventStore {
    events: RwLock<Vec<MemoryEvent>>,
}

impl MockMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MemoryEventStore for MockMemoryEventStore {
    async fn record_event(
        &self,
        tenant: &TenantContext,
        event: MemoryEvent,
    ) -> MemcoreResult<MemoryEvent> {
        ensure_event_tenant(&event, tenant)?;
        self.events
            .write()
            .expect("events lock poisoned")
            .push(event.clone());
        Ok(event)
    }

    async fn list_events(&self, query: MemoryEventQuery) -> MemcoreResult<Vec<MemoryEvent>> {
        let _ = normalize_event_list_limit(query.limit)?;
        let events = self.events.read().expect("events lock poisoned");

        let mut results: Vec<MemoryEvent> = events
            .iter()
            .filter(|event| event_matches_tenant(event, &query.tenant))
            .filter(|event| {
                query
                    .fact_id
                    .is_none_or(|fact_id| event.fact_id == Some(fact_id))
            })
            .filter(|event| {
                query
                    .operation
                    .is_none_or(|operation| event.operation == operation)
            })
            .filter(|event| {
                query
                    .created_after
                    .is_none_or(|created_after| event.created_at >= created_after)
            })
            .filter(|event| {
                query
                    .created_before
                    .is_none_or(|created_before| event.created_at < created_before)
            })
            .filter(|event| {
                query.cursor.as_ref().is_none_or(|cursor| {
                    is_after_cursor_in_desc_order(
                        event.created_at,
                        &event.id.to_string(),
                        cursor,
                    )
                })
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        results.truncate(page_fetch_limit(query.limit));
        Ok(results)
    }

    async fn list_events_by_org(
        &self,
        query: OrgMemoryEventQuery,
    ) -> MemcoreResult<Vec<MemoryEvent>> {
        let limit = normalize_event_list_limit(query.limit)?;
        let events = self.events.read().expect("events lock poisoned");

        let mut results: Vec<MemoryEvent> = events
            .iter()
            .filter(|event| event.org_id == query.org_id)
            .filter(|event| {
                query
                    .user_id
                    .as_ref()
                    .is_none_or(|user_id| event.user_id == *user_id)
            })
            .filter(|event| {
                query
                    .fact_id
                    .is_none_or(|fact_id| event.fact_id == Some(fact_id))
            })
            .filter(|event| {
                query
                    .operation
                    .is_none_or(|operation| event.operation == operation)
            })
            .filter(|event| {
                query
                    .created_after
                    .is_none_or(|created_after| event.created_at >= created_after)
            })
            .filter(|event| {
                query
                    .created_before
                    .is_none_or(|created_before| event.created_at < created_before)
            })
            .filter(|event| {
                query.cursor.as_ref().is_none_or(|cursor| {
                    is_after_cursor_in_desc_order(
                        event.created_at,
                        &event.id.to_string(),
                        cursor,
                    )
                })
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        results.truncate(page_fetch_limit(limit));
        Ok(results)
    }

    async fn delete_events_older_than(
        &self,
        tenant: &TenantContext,
        cutoff: DateTime<Utc>,
        dry_run: bool,
    ) -> MemcoreResult<usize> {
        let mut events = self.events.write().expect("events lock poisoned");
        let before_len = events.len();

        if dry_run {
            let count = events
                .iter()
                .filter(|event| event_matches_tenant(event, tenant))
                .filter(|event| event.created_at < cutoff)
                .count();
            return Ok(count);
        }

        events.retain(|event| {
            !(event_matches_tenant(event, tenant) && event.created_at < cutoff)
        });

        Ok(before_len - events.len())
    }

    async fn count_events_by_org(&self, org_id: &str) -> MemcoreResult<usize> {
        let events = self.events.read().expect("events lock poisoned");
        Ok(events.iter().filter(|event| event.org_id == org_id).count())
    }
}

#[derive(Debug, Default)]
pub struct MockApiKeyStore {
    keys: RwLock<Vec<ApiKeyRecord>>,
}

impl MockApiKeyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ApiKeyStore for MockApiKeyStore {
    async fn find_by_hash(&self, key_hash: &str) -> MemcoreResult<Option<ApiKeyRecord>> {
        let keys = self.keys.read().expect("api keys lock poisoned");
        Ok(keys
            .iter()
            .find(|record| record.key_hash == key_hash && record.is_active())
            .cloned())
    }

    async fn insert_api_key(&self, record: ApiKeyRecord) -> MemcoreResult<ApiKeyRecord> {
        self.keys
            .write()
            .expect("api keys lock poisoned")
            .push(record.clone());
        Ok(record)
    }

    async fn revoke_api_key(&self, org_id: &str, key_id: Uuid) -> MemcoreResult<()> {
        let mut keys = self.keys.write().expect("api keys lock poisoned");
        let Some(record) = keys
            .iter_mut()
            .find(|record| record.id == key_id && record.org_id == org_id && record.is_active())
        else {
            return Err(MemcoreError::NotFound(format!("api key not found: {key_id}")));
        };

        record.revoked_at = Some(Utc::now());
        Ok(())
    }

    async fn list_api_keys(
        &self,
        query: ApiKeyListQuery,
    ) -> MemcoreResult<Vec<ApiKeyRecord>> {
        let keys = self.keys.read().expect("api keys lock poisoned");
        let mut results: Vec<ApiKeyRecord> = keys
            .iter()
            .filter(|record| record.org_id == query.org_id)
            .filter(|record| query.include_revoked || record.is_active())
            .filter(|record| {
                query.cursor.as_ref().is_none_or(|cursor| {
                    is_after_cursor_in_desc_order(
                        record.created_at,
                        &record.id.to_string(),
                        cursor,
                    )
                })
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        results.truncate(page_fetch_limit(query.limit));
        Ok(results)
    }
}

#[cfg(test)]
mod api_key_store_tests {
    use chrono::Utc;
    use memcore_common::hash_api_key;
    use memcore_core::ApiKeyScope;
    use uuid::Uuid;

    use super::MockApiKeyStore;
    use crate::traits::ApiKeyStore;
    use memcore_core::{ApiKeyListQuery, ApiKeyRecord};

    #[tokio::test]
    async fn mock_list_api_keys_by_org() {
        let store = MockApiKeyStore::new();
        let active = ApiKeyRecord {
            id: Uuid::new_v4(),
            org_id: "org_a".to_string(),
            name: "active".to_string(),
            key_hash: hash_api_key("pepper", "active"),
            scopes: vec![ApiKeyScope::MemoryRead],
            created_at: Utc::now(),
            revoked_at: None,
        };
        let revoked = ApiKeyRecord {
            id: Uuid::new_v4(),
            org_id: "org_a".to_string(),
            name: "revoked".to_string(),
            key_hash: hash_api_key("pepper", "revoked"),
            scopes: vec![ApiKeyScope::MemoryRead],
            created_at: Utc::now(),
            revoked_at: Some(Utc::now()),
        };
        let other_org = ApiKeyRecord {
            id: Uuid::new_v4(),
            org_id: "org_b".to_string(),
            name: "other".to_string(),
            key_hash: hash_api_key("pepper", "other"),
            scopes: vec![ApiKeyScope::MemoryRead],
            created_at: Utc::now(),
            revoked_at: None,
        };

        store.insert_api_key(active).await.expect("insert");
        store.insert_api_key(revoked).await.expect("insert");
        store.insert_api_key(other_org).await.expect("insert");

        let active_only = store
            .list_api_keys(ApiKeyListQuery {
                org_id: "org_a".to_string(),
                include_revoked: false,
                limit: 100,
                cursor: None,
            })
            .await
            .expect("list");
        assert_eq!(active_only.len(), 1);
        assert_eq!(active_only[0].name, "active");

        let with_revoked = store
            .list_api_keys(ApiKeyListQuery {
                org_id: "org_a".to_string(),
                include_revoked: true,
                limit: 100,
                cursor: None,
            })
            .await
            .expect("list");
        assert_eq!(with_revoked.len(), 2);

        let org_b = store
            .list_api_keys(ApiKeyListQuery {
                org_id: "org_b".to_string(),
                include_revoked: false,
                limit: 100,
                cursor: None,
            })
            .await
            .expect("list");
        assert_eq!(org_b.len(), 1);
        assert_eq!(org_b[0].org_id, "org_b");
    }

    #[tokio::test]
    async fn mock_api_key_store_insert_find_revoke() {
        let store = MockApiKeyStore::new();
        let record = ApiKeyRecord {
            id: Uuid::new_v4(),
            org_id: "org_mock".to_string(),
            name: "mock".to_string(),
            key_hash: hash_api_key("pepper", "token"),
            scopes: vec![ApiKeyScope::MemoryRead],
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
        assert_eq!(found.id, record.id);

        store
            .revoke_api_key("org_mock", record.id)
            .await
            .expect("revoke should succeed");
        assert!(store
            .find_by_hash(&record.key_hash)
            .await
            .expect("find should succeed")
            .is_none());
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use memcore_core::{MemorySource, MemoryType, OrgUserListQuery};
    use serde_json::json;
    use uuid::Uuid;

    use super::{MockFactStore, MockVectorStore};
    use crate::queries::FactSearchQuery;
    use crate::traits::{FactStore, VectorStore};
    use crate::vector::{VectorRecord, VectorSearchQuery};

    fn tenant(org_id: &str, user_id: &str) -> memcore_core::TenantContext {
        memcore_core::TenantContext::new(org_id, user_id).expect("tenant should be valid")
    }

    fn sample_fact(
        org_id: &str,
        user_id: &str,
        content: &str,
        memory_type: MemoryType,
    ) -> memcore_core::Fact {
        let now = Utc::now();
        memcore_core::Fact::new(
            Uuid::new_v4(),
            org_id,
            user_id,
            memory_type,
            content,
            None,
            MemorySource::UserMessage,
            0.9,
            0.8,
            None,
            None,
            now,
            now,
            json!({}),
        )
        .expect("fact should be valid")
    }

    fn sample_vector(
        org_id: &str,
        user_id: &str,
        fact_id: Uuid,
        embedding: Vec<f32>,
        content: &str,
        memory_type: MemoryType,
    ) -> VectorRecord {
        VectorRecord {
            id: Uuid::new_v4(),
            fact_id,
            org_id: org_id.to_string(),
            user_id: user_id.to_string(),
            embedding,
            content: content.to_string(),
            memory_type,
            metadata: json!({ "topic": "rust" }),
        }
    }

    #[tokio::test]
    async fn insert_and_get_fact() {
        let store = MockFactStore::new();
        let tenant = tenant("org_a", "user_a");
        let fact = sample_fact("org_a", "user_a", "learning rust", MemoryType::Skill);

        let inserted = store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");
        assert_eq!(inserted.id, fact.id);

        let fetched = store
            .get_fact(&tenant, fact.id)
            .await
            .expect("get should succeed")
            .expect("fact should exist");
        assert_eq!(fetched.content, "learning rust");
    }

    #[tokio::test]
    async fn tenant_isolation_for_facts() {
        let store = MockFactStore::new();
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");
        let fact = sample_fact("org_a", "user_a", "private memory", MemoryType::Profile);

        store
            .insert_fact(&tenant_a, fact.clone())
            .await
            .expect("insert should succeed");

        let cross_tenant = store
            .get_fact(&tenant_b, fact.id)
            .await
            .expect("get should succeed");
        assert!(cross_tenant.is_none());
    }

    #[tokio::test]
    async fn search_facts_by_tenant() {
        let store = MockFactStore::new();
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_b", "user_b");

        store
            .insert_fact(
                &tenant_a,
                sample_fact("org_a", "user_a", "rust backend", MemoryType::Skill),
            )
            .await
            .expect("insert should succeed");
        store
            .insert_fact(
                &tenant_b,
                sample_fact("org_b", "user_b", "rust backend", MemoryType::Skill),
            )
            .await
            .expect("insert should succeed");

        let query = FactSearchQuery {
            tenant: tenant_a,
            memory_types: None,
            query_text: Some("rust".to_string()),
            limit: 10,
            cursor: None,
            include_deleted: false,
        };

        let results = store
            .search_facts(query)
            .await
            .expect("search should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].org_id, "org_a");
    }

    #[tokio::test]
    async fn soft_delete_fact_hides_from_get() {
        let store = MockFactStore::new();
        let tenant = tenant("org_a", "user_a");
        let fact = sample_fact("org_a", "user_a", "to delete", MemoryType::Task);

        store
            .insert_fact(&tenant, fact.clone())
            .await
            .expect("insert should succeed");
        store
            .soft_delete_fact(&tenant, fact.id)
            .await
            .expect("soft delete should succeed");

        let fetched = store
            .get_fact(&tenant, fact.id)
            .await
            .expect("get should succeed");
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn retention_dry_run_counts_old_facts_without_deleting() {
        let store = MockFactStore::new();
        let tenant = tenant("org_a", "user_a");
        let cutoff = Utc::now() - chrono::Duration::days(30);

        let mut old_fact = sample_fact("org_a", "user_a", "old", MemoryType::Profile);
        old_fact.updated_at = Utc::now() - chrono::Duration::days(60);
        store
            .insert_fact(&tenant, old_fact)
            .await
            .expect("insert old fact");

        store
            .insert_fact(
                &tenant,
                sample_fact("org_a", "user_a", "recent", MemoryType::Profile),
            )
            .await
            .expect("insert recent fact");

        let result = store
            .delete_facts_older_than(&tenant, cutoff, true)
            .await
            .expect("dry-run should succeed");

        assert_eq!(result.count, 1);
        assert!(result.fact_ids.is_empty());

        let listed = store
            .search_facts(FactSearchQuery::new(tenant, 10))
            .await
            .expect("search");
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn retention_apply_soft_deletes_only_matching_tenant() {
        let store = MockFactStore::new();
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");
        let cutoff = Utc::now() - chrono::Duration::days(30);

        let mut old_a = sample_fact("org_a", "user_a", "old a", MemoryType::Profile);
        old_a.updated_at = Utc::now() - chrono::Duration::days(60);
        store.insert_fact(&tenant_a, old_a).await.expect("insert");

        let mut old_b = sample_fact("org_a", "user_b", "old b", MemoryType::Profile);
        old_b.updated_at = Utc::now() - chrono::Duration::days(60);
        store.insert_fact(&tenant_b, old_b).await.expect("insert");

        let result = store
            .delete_facts_older_than(&tenant_a, cutoff, false)
            .await
            .expect("apply should succeed");

        assert_eq!(result.count, 1);

        let remaining_b = store
            .search_facts(FactSearchQuery::new(tenant_b, 10))
            .await
            .expect("search b");
        assert_eq!(remaining_b.len(), 1);
    }

    #[tokio::test]
    async fn delete_user_data_removes_tenant_facts() {
        let store = MockFactStore::new();
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");

        store
            .insert_fact(
                &tenant_a,
                sample_fact("org_a", "user_a", "user a memory", MemoryType::Profile),
            )
            .await
            .expect("insert should succeed");
        store
            .insert_fact(
                &tenant_b,
                sample_fact("org_a", "user_b", "user b memory", MemoryType::Profile),
            )
            .await
            .expect("insert should succeed");

        store
            .delete_user_data(&tenant_a)
            .await
            .expect("delete user data should succeed");

        let query_a = FactSearchQuery::new(tenant_a, 10);
        let query_b = FactSearchQuery::new(tenant_b, 10);

        assert!(store.search_facts(query_a).await.expect("search").is_empty());
        assert_eq!(
            store.search_facts(query_b).await.expect("search").len(),
            1
        );
    }

    #[tokio::test]
    async fn upsert_and_search_vector() {
        let store = MockVectorStore::new();
        let tenant = tenant("org_a", "user_a");
        let fact_id = Uuid::new_v4();
        let record = sample_vector(
            "org_a",
            "user_a",
            fact_id,
            vec![1.0, 0.0, 0.0],
            "rust vector",
            MemoryType::Skill,
        );

        store
            .upsert_vector(&tenant, record)
            .await
            .expect("upsert should succeed");

        let query = VectorSearchQuery {
            tenant,
            embedding: vec![1.0, 0.0, 0.0],
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        };

        let results = store
            .search_vectors(query)
            .await
            .expect("search should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact_id, fact_id);
        assert!(results[0].score > 0.99);
    }

    #[tokio::test]
    async fn tenant_isolation_for_vectors() {
        let store = MockVectorStore::new();
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");
        let fact_id = Uuid::new_v4();

        store
            .upsert_vector(
                &tenant_a,
                sample_vector(
                    "org_a",
                    "user_a",
                    fact_id,
                    vec![0.0, 1.0, 0.0],
                    "private vector",
                    MemoryType::Preference,
                ),
            )
            .await
            .expect("upsert should succeed");

        let query = VectorSearchQuery {
            tenant: tenant_b,
            embedding: vec![0.0, 1.0, 0.0],
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        };

        let results = store
            .search_vectors(query)
            .await
            .expect("search should succeed");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn delete_vector_by_fact_id() {
        let store = MockVectorStore::new();
        let tenant = tenant("org_a", "user_a");
        let fact_id = Uuid::new_v4();
        let record = sample_vector(
            "org_a",
            "user_a",
            fact_id,
            vec![1.0, 1.0, 0.0],
            "delete me",
            MemoryType::Entity,
        );

        store
            .upsert_vector(&tenant, record)
            .await
            .expect("upsert should succeed");
        store
            .delete_by_fact_id(&tenant, fact_id)
            .await
            .expect("delete by fact id should succeed");

        let query = VectorSearchQuery {
            tenant,
            embedding: vec![1.0, 1.0, 0.0],
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        };

        assert!(store
            .search_vectors(query)
            .await
            .expect("search should succeed")
            .is_empty());
    }

    #[tokio::test]
    async fn delete_vectors_by_user() {
        let store = MockVectorStore::new();
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");

        store
            .upsert_vector(
                &tenant_a,
                sample_vector(
                    "org_a",
                    "user_a",
                    Uuid::new_v4(),
                    vec![1.0, 0.0],
                    "a",
                    MemoryType::System,
                ),
            )
            .await
            .expect("upsert should succeed");
        store
            .upsert_vector(
                &tenant_b,
                sample_vector(
                    "org_a",
                    "user_b",
                    Uuid::new_v4(),
                    vec![0.0, 1.0],
                    "b",
                    MemoryType::System,
                ),
            )
            .await
            .expect("upsert should succeed");

        store
            .delete_by_user(&tenant_a)
            .await
            .expect("delete by user should succeed");

        let query_a = VectorSearchQuery {
            tenant: tenant_a,
            embedding: vec![1.0, 0.0],
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        };
        let query_b = VectorSearchQuery {
            tenant: tenant_b,
            embedding: vec![0.0, 1.0],
            limit: 5,
            memory_types: None,
            metadata_filter: None,
        };

        assert!(store
            .search_vectors(query_a)
            .await
            .expect("search")
            .is_empty());
        assert_eq!(
            store
                .search_vectors(query_b)
                .await
                .expect("search")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn memory_event_store_enforces_tenant_on_record() {
        use super::MockMemoryEventStore;
        use memcore_common::MemcoreError;
        use memcore_core::MemoryEventOperation;
        use crate::traits::MemoryEventStore;

        let store = MockMemoryEventStore::new();
        let tenant_a = tenant("org_a", "user_a");
        let tenant_b = tenant("org_a", "user_b");
        let event = memcore_core::MemoryEvent::new(
            tenant_a.org_id.clone(),
            tenant_a.user_id.clone(),
            None,
            MemoryEventOperation::Add,
            None,
            Some("content".to_string()),
            None,
            None,
            json!({}),
        );

        store
            .record_event(&tenant_a, event.clone())
            .await
            .expect("record should succeed");

        let error = store
            .record_event(&tenant_b, event)
            .await
            .expect_err("cross-tenant record should fail");
        assert_eq!(error, MemcoreError::Forbidden);
    }

    #[tokio::test]
    async fn org_admin_mock_counts_are_org_scoped() {
        let store = MockFactStore::new();
        let tenant_a = tenant("org_mock_admin", "user_a");
        let tenant_b = tenant("org_mock_admin", "user_b");
        let other_org = tenant("org_other", "user_x");

        store
            .insert_fact(
                &tenant_a,
                sample_fact("org_mock_admin", "user_a", "one", MemoryType::Profile),
            )
            .await
            .expect("insert");
        store
            .insert_fact(
                &tenant_b,
                sample_fact("org_mock_admin", "user_b", "two", MemoryType::Profile),
            )
            .await
            .expect("insert");
        store
            .insert_fact(
                &other_org,
                sample_fact("org_other", "user_x", "other", MemoryType::Profile),
            )
            .await
            .expect("insert");

        assert_eq!(
            store
                .count_facts_by_org("org_mock_admin")
                .await
                .expect("count facts"),
            2
        );
        assert_eq!(
            store
                .count_users_by_org("org_mock_admin")
                .await
                .expect("count users"),
            2
        );

        let users = store
            .list_users_by_org(OrgUserListQuery {
                org_id: "org_mock_admin".to_string(),
                limit: 1,
                cursor: None,
            })
            .await
            .expect("list users");
        // Store over-fetches by one for cursor pagination; engine trims to `limit`.
        assert_eq!(users.len(), 2);
    }

    #[tokio::test]
    async fn org_audit_mock_list_events_by_org() {
        use super::MockMemoryEventStore;
        use memcore_core::MemoryEventOperation;
        use memcore_core::ports::OrgMemoryEventQuery;
        use crate::traits::MemoryEventStore;

        let store = MockMemoryEventStore::new();
        let tenant_a = tenant("org_mock_audit", "user_a");
        let tenant_b = tenant("org_mock_audit", "user_b");
        let other_org = tenant("org_other", "user_x");
        let fact_id = Uuid::new_v4();

        store
            .record_event(
                &tenant_a,
                memcore_core::MemoryEvent::new(
                    tenant_a.org_id.clone(),
                    tenant_a.user_id.clone(),
                    Some(fact_id),
                    MemoryEventOperation::Update,
                    None,
                    None,
                    None,
                    None,
                    serde_json::json!({}),
                ),
            )
            .await
            .expect("record");
        store
            .record_event(
                &tenant_b,
                memcore_core::MemoryEvent::new(
                    tenant_b.org_id.clone(),
                    tenant_b.user_id.clone(),
                    None,
                    MemoryEventOperation::Add,
                    None,
                    None,
                    None,
                    None,
                    serde_json::json!({}),
                ),
            )
            .await
            .expect("record");
        store
            .record_event(
                &other_org,
                memcore_core::MemoryEvent::new(
                    other_org.org_id.clone(),
                    other_org.user_id.clone(),
                    None,
                    MemoryEventOperation::Add,
                    None,
                    None,
                    None,
                    None,
                    serde_json::json!({}),
                ),
            )
            .await
            .expect("record");

        let events = store
            .list_events_by_org(OrgMemoryEventQuery::new(
                "org_mock_audit".to_string(),
                10,
            ))
            .await
            .expect("list");
        assert_eq!(events.len(), 2);
    }
}
