use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use memcore_common::{MemcoreError, MemcoreResult};
use tokio::sync::Mutex;
use tokio::time;

use super::cache::{
    CachedContextEntry, ContextCache, ContextCacheConfig, ContextCacheKey, ContextCacheUsage,
};

/// Process-local stampede protection configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextCacheStampedeConfig {
    pub enabled: bool,
    pub lock_timeout: Duration,
}

impl ContextCacheStampedeConfig {
    pub fn from_cache_config(config: &ContextCacheConfig) -> Self {
        Self {
            enabled: config.stampede_protection_active(),
            lock_timeout: Duration::from_secs(config.stampede_lock_timeout_seconds),
        }
    }

    pub fn validate(&self) -> MemcoreResult<()> {
        if self.enabled && self.lock_timeout.is_zero() {
            return Err(MemcoreError::ValidationError(
                "context cache stampede lock timeout must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}

/// Coordinates cache lookups with optional in-process request coalescing.
pub struct ContextCacheCoordinator {
    cache: Arc<dyn ContextCache>,
    cache_config: ContextCacheConfig,
    stampede_config: ContextCacheStampedeConfig,
    inflight: Arc<Mutex<HashMap<ContextCacheKey, Arc<Mutex<()>>>>>,
}

impl ContextCacheCoordinator {
    pub fn new(cache: Arc<dyn ContextCache>, cache_config: ContextCacheConfig) -> Self {
        let stampede_config = ContextCacheStampedeConfig::from_cache_config(&cache_config);
        Self {
            cache,
            cache_config,
            stampede_config,
            inflight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn cache_config(&self) -> &ContextCacheConfig {
        &self.cache_config
    }

    #[cfg(test)]
    pub(crate) fn inflight_lock_count(&self) -> usize {
        self.inflight
            .try_lock()
            .map(|map| map.len())
            .unwrap_or(0)
    }

    pub async fn get_or_compute<F, Fut>(
        &self,
        key: ContextCacheKey,
        compute: F,
    ) -> MemcoreResult<(CachedContextEntry, ContextCacheUsage)>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = MemcoreResult<CachedContextEntry>>,
    {
        self.cache_config.validate()?;
        self.stampede_config.validate()?;

        if let Some(entry) = self.cache.get(&key).await? {
            return Ok((entry, ContextCacheUsage::hit(&self.cache_config)));
        }

        if !self.stampede_config.enabled {
            let entry = compute().await?;
            self.cache.set(key, entry.clone()).await?;
            return Ok((entry, ContextCacheUsage::miss(&self.cache_config)));
        }

        let lock_arc = self.lock_for_key(&key).await;
        let waited_for_inflight = !lock_arc.try_lock().is_ok();
        let _guard = if waited_for_inflight {
            match time::timeout(self.stampede_config.lock_timeout, lock_arc.lock()).await {
                Ok(guard) => guard,
                Err(_) => {
                    return Err(MemcoreError::Timeout(
                        "timed out waiting for context cache computation".to_string(),
                    ));
                }
            }
        } else {
            lock_arc.lock().await
        };

        if let Some(entry) = self.cache.get(&key).await? {
            self.cleanup_inflight_lock(&key, &lock_arc).await;
            return Ok((
                entry,
                ContextCacheUsage::hit_with_wait(&self.cache_config, waited_for_inflight),
            ));
        }

        let compute_result = compute().await;
        match compute_result {
            Ok(entry) => {
                self.cache.set(key.clone(), entry.clone()).await?;
                self.cleanup_inflight_lock(&key, &lock_arc).await;
                Ok((entry, ContextCacheUsage::miss(&self.cache_config)))
            }
            Err(error) => {
                self.cleanup_inflight_lock(&key, &lock_arc).await;
                Err(error)
            }
        }
    }

    async fn lock_for_key(&self, key: &ContextCacheKey) -> Arc<Mutex<()>> {
        let mut map = self.inflight.lock().await;
        map.entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn cleanup_inflight_lock(&self, key: &ContextCacheKey, lock_arc: &Arc<Mutex<()>>) {
        let mut map = self.inflight.lock().await;
        if let Some(existing) = map.get(key) {
            if Arc::ptr_eq(existing, lock_arc) {
                map.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::{Duration as ChronoDuration, Utc};
    use tokio::sync::Barrier;

    use super::*;
    use crate::context::budget::ContextBudgetUsage;
    use crate::context::compression_options::ContextCompressionUsage;
    use crate::context::format_options::ContextFormat;
    use crate::context::{build_context_cache_key, InMemoryContextCache};
    use crate::context::types::BuildContextInput;
    use crate::TenantContext;

    fn cache_config(stampede_enabled: bool) -> ContextCacheConfig {
        ContextCacheConfig {
            enabled: true,
            ttl_seconds: 300,
            max_entries: 100,
            stampede_protection_enabled: stampede_enabled,
            stampede_lock_timeout_seconds: 30,
        }
    }

    fn sample_input(org_id: &str, user_id: &str, query: &str) -> BuildContextInput {
        BuildContextInput {
            tenant: TenantContext::new(org_id, user_id).expect("tenant"),
            query: query.to_string(),
            ..Default::default()
        }
    }

    fn sample_entry(context: &str) -> CachedContextEntry {
        let now = Utc::now();
        CachedContextEntry {
            context: context.to_string(),
            memories: Vec::new(),
            created_at: now,
            expires_at: now + ChronoDuration::seconds(300),
            budget: ContextBudgetUsage {
                max_tokens: 2000,
                reserved_tokens: 300,
                available_tokens: 1700,
                used_tokens: 10,
                included_memories: 0,
                skipped_memories: 0,
            },
            compression: ContextCompressionUsage::disabled(),
        }
    }

    fn coordinator(stampede_enabled: bool) -> ContextCacheCoordinator {
        let config = cache_config(stampede_enabled);
        ContextCacheCoordinator::new(Arc::new(InMemoryContextCache::new(100)), config)
    }

    #[tokio::test]
    async fn cache_hit_returns_immediately_without_compute() {
        let cache = Arc::new(InMemoryContextCache::new(100));
        let config = cache_config(true);
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "hit"));
        cache
            .set(key.clone(), sample_entry("cached"))
            .await
            .expect("seed cache");
        let coordinator = ContextCacheCoordinator::new(cache, config);

        let compute_count = Arc::new(AtomicUsize::new(0));
        let count = compute_count.clone();
        let (entry, usage) = coordinator
            .get_or_compute(key, || async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(sample_entry("computed"))
            })
            .await
            .expect("cache hit");

        assert_eq!(entry.context, "cached");
        assert!(usage.hit);
        assert!(!usage.waited_for_inflight);
        assert_eq!(compute_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cache_miss_computes_once_and_stores_entry() {
        let coordinator = coordinator(true);
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "miss"));
        let compute_count = Arc::new(AtomicUsize::new(0));
        let count = compute_count.clone();

        let (entry, usage) = coordinator
            .get_or_compute(key.clone(), || async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(sample_entry("computed once"))
            })
            .await
            .expect("compute");

        assert_eq!(entry.context, "computed once");
        assert!(!usage.hit);
        assert_eq!(compute_count.load(Ordering::SeqCst), 1);
        let (entry, usage) = coordinator
            .get_or_compute(key, || async { Ok(sample_entry("ignored")) })
            .await
            .expect("second hit");
        assert_eq!(entry.context, "computed once");
        assert!(usage.hit);
    }

    #[tokio::test]
    async fn concurrent_identical_requests_compute_only_once() {
        let coordinator = Arc::new(coordinator(true));
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "concurrent"));
        let compute_count = Arc::new(AtomicUsize::new(0));
        let compute_started = Arc::new(tokio::sync::Notify::new());

        let coordinator_first = coordinator.clone();
        let key_first = key.clone();
        let compute_count_first = compute_count.clone();
        let compute_started_first = compute_started.clone();
        let first = tokio::spawn(async move {
            coordinator_first
                .get_or_compute(key_first, || async move {
                    compute_count_first.fetch_add(1, Ordering::SeqCst);
                    compute_started_first.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(sample_entry("shared"))
                })
                .await
        });

        compute_started.notified().await;
        let second = coordinator
            .get_or_compute(key, || async {
                compute_count.fetch_add(1, Ordering::SeqCst);
                Ok(sample_entry("should not run"))
            })
            .await
            .expect("second request");

        let first = first.await.expect("join first").expect("first request");

        assert_eq!(compute_count.load(Ordering::SeqCst), 1);
        assert_eq!(first.0.context, "shared");
        assert_eq!(second.0.context, "shared");
        assert!(second.1.hit);
        assert!(second.1.waited_for_inflight);
    }

    #[tokio::test]
    async fn waiting_request_reports_waited_for_inflight_on_hit() {
        let coordinator = Arc::new(coordinator(true));
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "wait hit"));
        let compute_started = Arc::new(tokio::sync::Notify::new());

        let coordinator_first = coordinator.clone();
        let key_first = key.clone();
        let compute_started_first = compute_started.clone();
        let first = tokio::spawn(async move {
            coordinator_first
                .get_or_compute(key_first, || async move {
                    compute_started_first.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(sample_entry("from first"))
                })
                .await
        });

        compute_started.notified().await;
        let (_, usage) = coordinator
            .get_or_compute(key, || async { Ok(sample_entry("should not run")) })
            .await
            .expect("waiter");
        let _ = first.await.expect("first join");

        assert!(usage.hit);
        assert!(usage.waited_for_inflight);
    }

    #[tokio::test]
    async fn different_query_keys_compute_independently() {
        let coordinator = coordinator(true);
        let key_a = build_context_cache_key(&sample_input("org_a", "user_a", "alpha"));
        let key_b = build_context_cache_key(&sample_input("org_a", "user_a", "beta"));
        let compute_count = Arc::new(AtomicUsize::new(0));

        for key in [key_a, key_b] {
            let count = compute_count.clone();
            coordinator
                .get_or_compute(key, || async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(sample_entry("x"))
                })
                .await
                .expect("compute");
        }

        assert_eq!(compute_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn different_users_and_orgs_compute_independently() {
        let coordinator = coordinator(true);
        let keys = [
            build_context_cache_key(&sample_input("org_a", "user_a", "same")),
            build_context_cache_key(&sample_input("org_a", "user_b", "same")),
            build_context_cache_key(&sample_input("org_b", "user_a", "same")),
        ];
        let compute_count = Arc::new(AtomicUsize::new(0));

        for key in keys {
            let count = compute_count.clone();
            coordinator
                .get_or_compute(key, || async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(sample_entry("x"))
                })
                .await
                .expect("compute");
        }

        assert_eq!(compute_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn different_options_compute_independently() {
        let coordinator = coordinator(true);
        let mut markdown = sample_input("org_a", "user_a", "same");
        markdown.format_options.format = ContextFormat::Markdown;
        let keys = [
            build_context_cache_key(&sample_input("org_a", "user_a", "same")),
            build_context_cache_key(&markdown),
        ];
        let compute_count = Arc::new(AtomicUsize::new(0));

        for key in keys {
            let count = compute_count.clone();
            coordinator
                .get_or_compute(key, || async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(sample_entry("x"))
                })
                .await
                .expect("compute");
        }

        assert_eq!(compute_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn compute_error_is_not_cached_and_later_request_can_recompute() {
        let coordinator = coordinator(true);
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "error"));
        let compute_count = Arc::new(AtomicUsize::new(0));

        let count = compute_count.clone();
        let error = coordinator
            .get_or_compute(key.clone(), || async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err(MemcoreError::Internal("compute failed".to_string()))
            })
            .await
            .expect_err("compute error");
        assert_eq!(error.code(), "internal");

        let count = compute_count.clone();
        let (entry, _) = coordinator
            .get_or_compute(key, || async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(sample_entry("recovered"))
            })
            .await
            .expect("recompute");

        assert_eq!(entry.context, "recovered");
        assert_eq!(compute_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn lock_timeout_returns_timeout_error() {
        let config = ContextCacheConfig {
            enabled: true,
            ttl_seconds: 300,
            max_entries: 100,
            stampede_protection_enabled: true,
            stampede_lock_timeout_seconds: 1,
        };
        let coordinator = Arc::new(ContextCacheCoordinator::new(
            Arc::new(InMemoryContextCache::new(100)),
            config,
        ));
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "timeout"));
        let hold = Arc::new(Barrier::new(2));

        let coordinator_bg = coordinator.clone();
        let key_bg = key.clone();
        let hold_bg = hold.clone();
        let _background = tokio::spawn(async move {
            let _ = coordinator_bg
                .get_or_compute(key_bg, || async {
                    hold_bg.wait().await;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(sample_entry("slow"))
                })
                .await;
        });

        hold.wait().await;
        let error = coordinator
            .get_or_compute(key, || async { Ok(sample_entry("never")) })
            .await
            .expect_err("timeout");

        assert_eq!(error.code(), "timeout");
        assert!(
            error
                .to_string()
                .contains("timed out waiting for context cache computation")
        );
    }

    #[tokio::test]
    async fn lock_map_cleaned_up_after_success() {
        let coordinator = coordinator(true);
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "cleanup"));
        assert_eq!(coordinator.inflight_lock_count(), 0);

        coordinator
            .get_or_compute(key, || async { Ok(sample_entry("done")) })
            .await
            .expect("compute");

        assert_eq!(coordinator.inflight_lock_count(), 0);
    }

    #[tokio::test]
    async fn stampede_protection_disabled_allows_independent_computes() {
        let coordinator = Arc::new(coordinator(false));
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "no stampede"));
        let compute_count = Arc::new(AtomicUsize::new(0));
        let compute_started = Arc::new(tokio::sync::Notify::new());

        let coordinator_first = coordinator.clone();
        let key_first = key.clone();
        let compute_count_first = compute_count.clone();
        let compute_started_first = compute_started.clone();
        let first = tokio::spawn(async move {
            coordinator_first
                .get_or_compute(key_first, || async move {
                    compute_count_first.fetch_add(1, Ordering::SeqCst);
                    compute_started_first.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(sample_entry("parallel"))
                })
                .await
        });

        compute_started.notified().await;
        let _ = coordinator
            .get_or_compute(key, || async {
                compute_count.fetch_add(1, Ordering::SeqCst);
                Ok(sample_entry("parallel two"))
            })
            .await
            .expect("second compute");

        let _ = first.await.expect("join first");
        assert_eq!(compute_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn many_concurrent_identical_requests_compute_only_once() {
        let coordinator = Arc::new(coordinator(true));
        let key = build_context_cache_key(&sample_input("org_a", "user_a", "many"));
        let compute_count = Arc::new(AtomicUsize::new(0));
        let compute_started = Arc::new(tokio::sync::Notify::new());
        let sleeping = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let coordinator = coordinator.clone();
            let key = key.clone();
            let compute_count = compute_count.clone();
            let compute_started = compute_started.clone();
            let sleeping = sleeping.clone();
            handles.push(tokio::spawn(async move {
                coordinator
                    .get_or_compute(key, || async move {
                        compute_count.fetch_add(1, Ordering::SeqCst);
                        if !sleeping.swap(true, Ordering::SeqCst) {
                            compute_started.notify_waiters();
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Ok(sample_entry("shared many"))
                    })
                    .await
            }));
        }

        compute_started.notified().await;
        for handle in handles {
            handle.await.expect("join").expect("compute");
        }

        assert_eq!(compute_count.load(Ordering::SeqCst), 1);
    }
}
