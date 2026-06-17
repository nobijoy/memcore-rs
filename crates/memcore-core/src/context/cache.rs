use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use hex;
use memcore_common::MemcoreResult;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::MemorySearchResult;
use crate::MemoryType;

use super::budget::ContextBudgetUsage;
use super::compression_options::{ContextCompressionOptions, ContextCompressionUsage};
use super::format_options::ContextFormatOptions;
use super::types::{BuildContextInput, BuildContextOutput};

/// Default context cache TTL in seconds.
pub const DEFAULT_CONTEXT_CACHE_TTL_SECONDS: u64 = 300;

/// Default maximum in-memory context cache entries.
pub const DEFAULT_CONTEXT_CACHE_MAX_ENTRIES: usize = 1000;

/// Tenant-scoped cache lookup key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContextCacheKey {
    pub org_id: String,
    pub user_id: String,
    pub query_hash: String,
    pub options_hash: String,
}

/// Cached assembled context payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedContextEntry {
    pub context: String,
    pub memories: Vec<MemorySearchResult>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub budget: ContextBudgetUsage,
    pub compression: ContextCompressionUsage,
}

/// Context cache configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextCacheConfig {
    pub enabled: bool,
    pub ttl_seconds: u64,
    pub max_entries: usize,
}

impl Default for ContextCacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_seconds: DEFAULT_CONTEXT_CACHE_TTL_SECONDS,
            max_entries: DEFAULT_CONTEXT_CACHE_MAX_ENTRIES,
        }
    }
}

impl ContextCacheConfig {
    pub fn validate(&self) -> MemcoreResult<()> {
        if !self.enabled {
            return Ok(());
        }

        if self.ttl_seconds == 0 {
            return Err(memcore_common::MemcoreError::ValidationError(
                "context cache ttl_seconds must be greater than 0 when cache is enabled".to_string(),
            ));
        }

        if self.max_entries == 0 {
            return Err(memcore_common::MemcoreError::ValidationError(
                "context cache max_entries must be greater than 0 when cache is enabled"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

/// Cache metadata returned with assembled context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCacheUsage {
    pub enabled: bool,
    pub hit: bool,
    pub ttl_seconds: Option<u64>,
}

impl ContextCacheUsage {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            hit: false,
            ttl_seconds: None,
        }
    }

    pub fn miss(config: &ContextCacheConfig) -> Self {
        Self {
            enabled: true,
            hit: false,
            ttl_seconds: Some(config.ttl_seconds),
        }
    }

    pub fn hit(config: &ContextCacheConfig) -> Self {
        Self {
            enabled: true,
            hit: true,
            ttl_seconds: Some(config.ttl_seconds),
        }
    }
}

/// Tenant-scoped context cache.
#[async_trait]
pub trait ContextCache: Send + Sync {
    async fn get(&self, key: &ContextCacheKey) -> MemcoreResult<Option<CachedContextEntry>>;

    async fn set(&self, key: ContextCacheKey, entry: CachedContextEntry) -> MemcoreResult<()>;

    async fn invalidate_user(&self, org_id: &str, user_id: &str) -> MemcoreResult<usize>;
}

/// Process-local in-memory context cache for dev/test use.
pub struct InMemoryContextCache {
    max_entries: usize,
    entries: Arc<RwLock<HashMap<ContextCacheKey, CachedContextEntry>>>,
}

impl InMemoryContextCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn purge_expired(map: &mut HashMap<ContextCacheKey, CachedContextEntry>, now: DateTime<Utc>) {
        map.retain(|_, entry| entry.expires_at > now);
    }

    fn evict_oldest(map: &mut HashMap<ContextCacheKey, CachedContextEntry>, max_entries: usize) {
        while map.len() > max_entries {
            let oldest_key = map
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest_key {
                map.remove(&key);
            } else {
                break;
            }
        }
    }
}

#[async_trait]
impl ContextCache for InMemoryContextCache {
    async fn get(&self, key: &ContextCacheKey) -> MemcoreResult<Option<CachedContextEntry>> {
        let now = Utc::now();
        let mut map = self
            .entries
            .write()
            .map_err(|_| lock_poisoned_error())?;

        if let Some(entry) = map.get(key) {
            if entry.expires_at <= now {
                map.remove(key);
                return Ok(None);
            }
            return Ok(Some(entry.clone()));
        }

        Ok(None)
    }

    async fn set(&self, key: ContextCacheKey, entry: CachedContextEntry) -> MemcoreResult<()> {
        let now = Utc::now();
        let mut map = self
            .entries
            .write()
            .map_err(|_| lock_poisoned_error())?;

        Self::purge_expired(&mut map, now);
        map.insert(key, entry);
        Self::evict_oldest(&mut map, self.max_entries);
        Ok(())
    }

    async fn invalidate_user(&self, org_id: &str, user_id: &str) -> MemcoreResult<usize> {
        let mut map = self
            .entries
            .write()
            .map_err(|_| lock_poisoned_error())?;

        let before = map.len();
        map.retain(|key, _| key.org_id != org_id || key.user_id != user_id);
        Ok(before.saturating_sub(map.len()))
    }
}

pub fn build_context_cache_key(input: &BuildContextInput) -> ContextCacheKey {
    ContextCacheKey {
        org_id: input.tenant.org_id.clone(),
        user_id: input.tenant.user_id.clone(),
        query_hash: stable_sha256_hex(&input.query),
        options_hash: stable_sha256_hex(&options_fingerprint_json(input)),
    }
}

pub fn cached_entry_from_output(
    output: &BuildContextOutput,
    ttl_seconds: u64,
) -> CachedContextEntry {
    let now = Utc::now();
    CachedContextEntry {
        context: output.context.clone(),
        memories: output.memories.clone(),
        budget: output.budget,
        compression: output.compression,
        created_at: now,
        expires_at: now + Duration::seconds(ttl_seconds as i64),
    }
}

fn options_fingerprint_json(input: &BuildContextInput) -> String {
    #[derive(Serialize)]
    struct Fingerprint<'a> {
        max_memories: usize,
        memory_types: Option<Vec<MemoryType>>,
        include_metadata: bool,
        max_tokens: usize,
        reserved_tokens: usize,
        format_options: &'a ContextFormatOptions,
        compression_options: &'a ContextCompressionOptions,
    }

    let fingerprint = Fingerprint {
        max_memories: input.max_memories,
        memory_types: input.memory_types.clone(),
        include_metadata: input.include_metadata,
        max_tokens: input.budget.max_tokens,
        reserved_tokens: input.budget.reserved_tokens,
        format_options: &input.format_options,
        compression_options: &input.compression_options,
    };

    serde_json::to_string(&fingerprint).unwrap_or_default()
}

pub fn stable_sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    hex::encode(digest)
}

fn lock_poisoned_error() -> memcore_common::MemcoreError {
    memcore_common::MemcoreError::ProviderError("context cache lock poisoned".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::budget::ContextBudget;
    use crate::context::compression_options::ContextCompressionMode;
    use crate::context::format_options::ContextFormat;
    use crate::TenantContext;

    fn sample_input(query: &str) -> BuildContextInput {
        BuildContextInput {
            tenant: TenantContext::new("org_a", "user_a").expect("tenant"),
            query: query.to_string(),
            ..Default::default()
        }
    }

    fn sample_output(context: &str) -> BuildContextOutput {
        BuildContextOutput {
            context: context.to_string(),
            memories: Vec::new(),
            budget: ContextBudgetUsage {
                max_tokens: 2000,
                reserved_tokens: 300,
                available_tokens: 1700,
                used_tokens: 10,
                included_memories: 0,
                skipped_memories: 0,
            },
            compression: ContextCompressionUsage::disabled(),
            cache: ContextCacheUsage::disabled(),
        }
    }

    #[tokio::test]
    async fn cache_set_and_get_works() {
        let cache = InMemoryContextCache::new(10);
        let key = build_context_cache_key(&sample_input("hello"));
        let entry = cached_entry_from_output(&sample_output("cached context"), 300);

        cache.set(key.clone(), entry.clone()).await.unwrap();
        let loaded = cache.get(&key).await.unwrap().expect("cache hit");
        assert_eq!(loaded.context, "cached context");
    }

    #[tokio::test]
    async fn expired_entry_returns_none_and_is_removed() {
        let cache = InMemoryContextCache::new(10);
        let key = build_context_cache_key(&sample_input("expired"));
        let mut entry = cached_entry_from_output(&sample_output("stale"), 300);
        entry.expires_at = Utc::now() - Duration::seconds(1);

        cache.set(key.clone(), entry).await.unwrap();
        assert!(cache.get(&key).await.unwrap().is_none());
        assert!(cache.get(&key).await.unwrap().is_none());
    }

    #[test]
    fn cache_key_includes_org_and_user() {
        let mut input = sample_input("same query");
        let base = build_context_cache_key(&input);

        input.tenant = TenantContext::new("org_b", "user_a").expect("tenant");
        let other_org = build_context_cache_key(&input);

        input.tenant = TenantContext::new("org_a", "user_b").expect("tenant");
        let other_user = build_context_cache_key(&input);

        assert_ne!(base.org_id, other_org.org_id);
        assert_ne!(base.user_id, other_user.user_id);
        assert_ne!(base, other_org);
        assert_ne!(base, other_user);
    }

    #[test]
    fn cache_key_changes_with_query_and_options() {
        let base = build_context_cache_key(&sample_input("alpha"));

        let query_changed = sample_input("beta");
        assert_ne!(base.query_hash, build_context_cache_key(&query_changed).query_hash);

        let mut format_changed = sample_input("alpha");
        format_changed.format_options.format = ContextFormat::Markdown;
        assert_ne!(base.options_hash, build_context_cache_key(&format_changed).options_hash);

        let mut budget_changed = sample_input("alpha");
        budget_changed.budget = ContextBudget {
            max_tokens: 500,
            reserved_tokens: 50,
        };
        assert_ne!(base.options_hash, build_context_cache_key(&budget_changed).options_hash);

        let mut compression_changed = sample_input("alpha");
        compression_changed.compression_options.mode = ContextCompressionMode::SimpleExtractive;
        assert_ne!(
            base.options_hash,
            build_context_cache_key(&compression_changed).options_hash
        );
    }

    #[tokio::test]
    async fn max_entries_eviction_works() {
        let cache = InMemoryContextCache::new(2);

        for index in 0..3 {
            let mut input = sample_input(&format!("query {index}"));
            input.tenant = TenantContext::new("org_a", &format!("user_{index}")).expect("tenant");
            let key = build_context_cache_key(&input);
            cache
                .set(
                    key,
                    cached_entry_from_output(&sample_output(&format!("ctx {index}")), 300),
                )
                .await
                .unwrap();
        }

        let remaining = cache.entries.read().expect("lock").len();
        assert_eq!(remaining, 2);
    }

    #[tokio::test]
    async fn invalidate_user_removes_only_matching_entries() {
        let cache = InMemoryContextCache::new(10);

        let key_a = build_context_cache_key(&sample_input("one"));
        let key_b = {
            let mut input = sample_input("two");
            input.tenant = TenantContext::new("org_b", "user_a").expect("tenant");
            build_context_cache_key(&input)
        };

        cache
            .set(key_a.clone(), cached_entry_from_output(&sample_output("a"), 300))
            .await
            .unwrap();
        cache
            .set(key_b.clone(), cached_entry_from_output(&sample_output("b"), 300))
            .await
            .unwrap();

        let removed = cache.invalidate_user("org_a", "user_a").await.unwrap();
        assert_eq!(removed, 1);
        assert!(cache.get(&key_a).await.unwrap().is_none());
        assert!(cache.get(&key_b).await.unwrap().is_some());
    }

    #[test]
    fn default_cache_config_is_disabled() {
        let config = ContextCacheConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.ttl_seconds, 300);
        assert_eq!(config.max_entries, 1000);
    }
}
