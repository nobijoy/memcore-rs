use std::sync::Arc;
use std::time::Duration;

use crate::middleware::RateLimiter;
use crate::observability::Metrics;
use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use memcore_config::{
    AuthMode, BackgroundJobLockBackend, ContextCacheBackend, DatabaseMigrationMode,
    EmbeddingProviderKind, EventBackend, FactBackend, LlmProviderKind, Settings, VectorBackend,
};
use memcore_core::{
    ApiKeyStore, BackgroundJob, BackgroundJobLockStore, BackgroundJobRetryPolicy,
    BackgroundJobRunStore, BackgroundJobRunner, ContextCacheConfig, EmbeddingProvider, FactStore,
    InMemoryContextCache, LlmProvider, MemoryEngine, MemoryEventStore, MemoryRetentionJob,
    MemoryUsageSnapshotJob, MemoryUsageSnapshotStore, OrgPlanStore, OrgQuotaLimits,
    ProviderUsageAttributionSlot, ProviderUsageRetentionJob, ProviderUsageStore, ShutdownToken,
    VectorStore,
};
use memcore_providers::{
    CircuitBreakerConfig, MockEmbeddingProvider, MockLlmProvider, OpenAiClient,
    OpenAiEmbeddingProvider, OpenAiLlmProvider, PersistentProviderUsageRecorder, ProviderCandidate,
    ProviderCapability, ProviderCircuitBreaker, ProviderExecutionPolicy, ProviderId,
    ProviderRoutingMetrics, ProviderUsageRecorder, build_resilient_embedding_from_candidates,
    build_resilient_llm_from_candidates, default_embedding_dimensions_for_model,
    new_token_usage_slot, provider_usage_recorder, validate_embedding_provider_name,
    validate_llm_provider_name, validate_provider_fallback_order,
    validate_summarizer_provider_name,
};
#[cfg(feature = "lancedb")]
use memcore_storage::LanceDbVectorStore;
#[cfg(feature = "qdrant")]
use memcore_storage::QdrantVectorStore;
#[cfg(feature = "redis-cache")]
use memcore_storage::RedisContextCache;
use memcore_storage::SqliteProviderUsageStore;
use memcore_storage::{
    MockApiKeyStore, MockBackgroundJobLockStore, MockBackgroundJobRunStore, MockFactStore,
    MockMemoryEventStore, MockMemoryUsageSnapshotStore, MockOrgPlanStore, MockProviderUsageStore,
    MockVectorStore, SqliteApiKeyStore, SqliteBackgroundJobLockStore, SqliteBackgroundJobRunStore,
    SqliteFactStore, SqliteMemoryEventStore, SqliteMemoryUsageSnapshotStore, SqliteOrgPlanStore,
};
#[cfg(feature = "postgres")]
use memcore_storage::{
    PostgresApiKeyStore, PostgresBackgroundJobLockStore, PostgresBackgroundJobRunStore,
    PostgresFactStore, PostgresMemoryEventStore, PostgresMemoryUsageSnapshotStore,
    PostgresOrgPlanStore, PostgresProviderUsageStore,
};
use memcore_storage::{
    StorageMigrationMode, StorageStartupCheckReport, check_sqlite_startup, connect_sqlite_pool,
};
#[cfg(feature = "postgres")]
use memcore_storage::{check_postgres_startup, connect_postgres_pool};

/// Default embedding dimensions for the mock embedding provider.
const MOCK_EMBEDDING_DIMENSIONS: usize = 8;

struct ProviderRuntime {
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    metrics: Arc<ProviderRoutingMetrics>,
    policy: ProviderExecutionPolicy,
}

/// Wires provider usage recording, optional persistence, and tenant attribution.
pub struct ProviderWiring {
    pub usage_recorder: Arc<dyn ProviderUsageRecorder>,
    pub usage_store: Option<Arc<dyn ProviderUsageStore>>,
    pub attribution_slot: Arc<ProviderUsageAttributionSlot>,
}

impl ProviderWiring {
    pub async fn from_settings(settings: &Settings) -> MemcoreResult<Self> {
        let inner = provider_usage_recorder(settings.provider_usage_metrics_enabled);
        let usage_store = if settings.provider_usage_persistence_enabled {
            Some(create_provider_usage_store(settings).await?)
        } else {
            None
        };
        let usage_recorder: Arc<dyn ProviderUsageRecorder> =
            if settings.provider_usage_persistence_enabled {
                PersistentProviderUsageRecorder::new(inner, usage_store.clone())
            } else {
                inner
            };

        Ok(Self {
            usage_recorder,
            usage_store,
            attribution_slot: Arc::new(ProviderUsageAttributionSlot::new()),
        })
    }

    pub fn for_tests(usage_recorder: Arc<dyn ProviderUsageRecorder>) -> Self {
        Self {
            usage_recorder,
            usage_store: None,
            attribution_slot: Arc::new(ProviderUsageAttributionSlot::new()),
        }
    }

    /// Synchronous wiring for mock-backend tests (no database I/O).
    pub fn for_mock_tests(settings: &Settings) -> Self {
        let inner = provider_usage_recorder(settings.provider_usage_metrics_enabled);
        if settings.provider_usage_persistence_enabled {
            let store = Arc::new(MockProviderUsageStore::new());
            Self {
                usage_recorder: PersistentProviderUsageRecorder::new(inner, Some(store.clone())),
                usage_store: Some(store),
                attribution_slot: Arc::new(ProviderUsageAttributionSlot::new()),
            }
        } else {
            Self::for_tests(inner)
        }
    }
}

async fn create_provider_usage_store(
    settings: &Settings,
) -> MemcoreResult<Arc<dyn ProviderUsageStore>> {
    if settings.fact_backend == FactBackend::Postgres {
        #[cfg(feature = "postgres")]
        {
            let url = require_postgres_url(settings)?;
            let store = PostgresProviderUsageStore::new(connect_postgres_pool(&url).await?);
            return Ok(Arc::new(store));
        }
        #[cfg(not(feature = "postgres"))]
        {
            return Err(MemcoreError::ValidationError(
                "provider usage persistence with postgres requires the `postgres` cargo feature"
                    .to_string(),
            ));
        }
    }

    if settings.fact_backend == FactBackend::Sqlite {
        let store = if is_sqlite_memory_database(settings) {
            SqliteProviderUsageStore::connect(&settings.database_url).await?
        } else {
            SqliteProviderUsageStore::new(connect_sqlite_pool(&settings.database_url).await?)
        };
        return Ok(Arc::new(store));
    }

    Ok(Arc::new(MockProviderUsageStore::new()))
}

async fn create_background_job_run_store(
    settings: &Settings,
) -> MemcoreResult<Option<Arc<dyn BackgroundJobRunStore>>> {
    if !settings.background_job_history_enabled {
        return Ok(None);
    }

    if settings.fact_backend == FactBackend::Postgres {
        #[cfg(feature = "postgres")]
        {
            let url = require_postgres_url(settings)?;
            let store = PostgresBackgroundJobRunStore::new(connect_postgres_pool(&url).await?);
            return Ok(Some(Arc::new(store)));
        }
        #[cfg(not(feature = "postgres"))]
        {
            return Err(MemcoreError::ValidationError(
                "background job history with postgres requires the `postgres` cargo feature"
                    .to_string(),
            ));
        }
    }

    if settings.fact_backend == FactBackend::Sqlite {
        let store = if is_sqlite_memory_database(settings) {
            SqliteBackgroundJobRunStore::connect(&settings.database_url).await?
        } else {
            SqliteBackgroundJobRunStore::new(connect_sqlite_pool(&settings.database_url).await?)
        };
        return Ok(Some(Arc::new(store)));
    }

    Ok(Some(Arc::new(MockBackgroundJobRunStore::new())))
}

async fn create_background_job_lock_store(
    settings: &Settings,
) -> MemcoreResult<Option<Arc<dyn BackgroundJobLockStore>>> {
    if !settings.background_job_lock_enabled {
        return Ok(None);
    }

    match settings.background_job_lock_backend {
        BackgroundJobLockBackend::Database => {}
    }

    if settings.fact_backend == FactBackend::Postgres {
        #[cfg(feature = "postgres")]
        {
            let url = require_postgres_url(settings)?;
            let store = PostgresBackgroundJobLockStore::new(connect_postgres_pool(&url).await?);
            return Ok(Some(Arc::new(store)));
        }
        #[cfg(not(feature = "postgres"))]
        {
            return Err(MemcoreError::ValidationError(
                "background job locks with postgres require the `postgres` cargo feature"
                    .to_string(),
            ));
        }
    }

    if settings.fact_backend == FactBackend::Sqlite {
        let store = if is_sqlite_memory_database(settings) {
            SqliteBackgroundJobLockStore::connect(&settings.database_url).await?
        } else {
            SqliteBackgroundJobLockStore::new(connect_sqlite_pool(&settings.database_url).await?)
        };
        return Ok(Some(Arc::new(store)));
    }

    Ok(Some(Arc::new(MockBackgroundJobLockStore::new())))
}

fn background_job_lock_owner_id(settings: &Settings) -> String {
    settings
        .background_job_lock_owner_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn provider_runtime(settings: &Settings) -> MemcoreResult<ProviderRuntime> {
    Ok(ProviderRuntime {
        circuit_breaker: Arc::new(ProviderCircuitBreaker::new(
            CircuitBreakerConfig::from_config(
                settings.provider_circuit_breaker_enabled,
                settings.provider_circuit_breaker_failure_threshold,
                settings.provider_circuit_breaker_reset_timeout_seconds,
                settings.provider_circuit_breaker_half_open_max_calls,
            )?,
        )),
        metrics: ProviderRoutingMetrics::new(),
        policy: provider_execution_policy(settings)?,
    })
}

fn llm_provider_kind_name(kind: &LlmProviderKind) -> &'static str {
    match kind {
        LlmProviderKind::Mock => "mock",
        LlmProviderKind::OpenAi => "openai",
        LlmProviderKind::OpenRouter => "openrouter",
        LlmProviderKind::Anthropic => "anthropic",
        LlmProviderKind::Groq => "groq",
    }
}

fn embedding_provider_kind_name(kind: &EmbeddingProviderKind) -> &'static str {
    match kind {
        EmbeddingProviderKind::Mock => "mock",
        EmbeddingProviderKind::OpenAi => "openai",
    }
}

fn llm_model_for_name(name: &str, settings: &Settings) -> String {
    match name {
        "mock" => settings.llm_model.clone(),
        "openai" => settings.llm_model.clone(),
        _ => settings.llm_model.clone(),
    }
}

fn embedding_model_for_name(name: &str, settings: &Settings) -> String {
    match name {
        "mock" => settings.embedding_model.clone(),
        "openai" => settings.embedding_model.clone(),
        _ => settings.embedding_model.clone(),
    }
}

fn build_llm_provider_by_name(
    name: &str,
    settings: &Settings,
) -> MemcoreResult<Arc<dyn LlmProvider>> {
    match name {
        "mock" => Ok(Arc::new(MockLlmProvider::new())),
        "openai" => {
            let api_key = require_openai_api_key(
                settings,
                "OPENAI_API_KEY is required when MEMCORE_LLM_PROVIDER=openai",
            )?;
            let client = OpenAiClient::new(&api_key, &settings.openai_base_url)
                .map_err(|err| provider_init_error("OpenAI LLM", err))?;
            Ok(Arc::new(OpenAiLlmProvider::new(
                client,
                settings.llm_model.clone(),
            )))
        }
        "openrouter" => Err(MemcoreError::ValidationError(
            "openrouter LLM provider is not wired into the API yet".to_string(),
        )),
        "anthropic" => Err(MemcoreError::ValidationError(
            "anthropic LLM provider is not wired into the API yet".to_string(),
        )),
        "groq" => Err(MemcoreError::ValidationError(
            "groq LLM provider is not wired into the API yet".to_string(),
        )),
        _ => Err(MemcoreError::ValidationError(format!(
            "unknown LLM provider in fallback order: {name}"
        ))),
    }
}

fn build_embedding_provider_by_name(
    name: &str,
    settings: &Settings,
) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    match name {
        "mock" => Ok(Arc::new(MockEmbeddingProvider::new(
            MOCK_EMBEDDING_DIMENSIONS,
        ))),
        "openai" => {
            let api_key = require_openai_api_key(
                settings,
                "OPENAI_API_KEY is required when MEMCORE_EMBEDDING_PROVIDER=openai",
            )?;
            let client = OpenAiClient::new(&api_key, &settings.openai_base_url)
                .map_err(|err| provider_init_error("OpenAI embedding", err))?;
            let dimensions = default_embedding_dimensions_for_model(&settings.embedding_model);
            let provider =
                OpenAiEmbeddingProvider::new(client, settings.embedding_model.clone(), dimensions)
                    .map_err(|err| provider_init_error("OpenAI embedding", err))?;
            Ok(Arc::new(provider))
        }
        _ => Err(MemcoreError::ValidationError(format!(
            "unknown embedding provider in fallback order: {name}"
        ))),
    }
}

fn llm_candidates_from_names(
    names: &[String],
    capability: ProviderCapability,
    settings: &Settings,
) -> MemcoreResult<Vec<ProviderCandidate<Arc<dyn LlmProvider>>>> {
    names
        .iter()
        .map(|name| {
            let usage_slot = new_token_usage_slot();
            Ok(ProviderCandidate::new(
                ProviderId::new(name.clone(), capability),
                build_llm_provider_by_name(name, settings)?,
                Some(llm_model_for_name(name, settings)),
                Some(usage_slot),
            ))
        })
        .collect()
}

fn embedding_candidates_from_names(
    names: &[String],
    settings: &Settings,
) -> MemcoreResult<Vec<ProviderCandidate<Arc<dyn EmbeddingProvider>>>> {
    names
        .iter()
        .map(|name| {
            let usage_slot = new_token_usage_slot();
            Ok(ProviderCandidate::new(
                ProviderId::new(name.clone(), ProviderCapability::Embedding),
                build_embedding_provider_by_name(name, settings)?,
                Some(embedding_model_for_name(name, settings)),
                Some(usage_slot),
            ))
        })
        .collect()
}

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub started_at: DateTime<Utc>,
    pub memory_engine: Arc<MemoryEngine>,
    pub api_key_store: Arc<dyn ApiKeyStore>,
    pub rate_limiter: Arc<RateLimiter>,
    pub metrics: Arc<Metrics>,
    pub provider_usage: Arc<dyn ProviderUsageRecorder>,
    pub provider_usage_store: Option<Arc<dyn ProviderUsageStore>>,
    pub org_plan_store: Arc<dyn OrgPlanStore>,
    pub background_jobs: Arc<BackgroundJobRunner>,
    pub background_job_run_store: Option<Arc<dyn BackgroundJobRunStore>>,
    pub background_job_lock_store: Option<Arc<dyn BackgroundJobLockStore>>,
    pub background_job_lock_owner_id: Option<String>,
    pub shutdown_token: ShutdownToken,
    pub storage_startup: StorageStartupCheckReport,
}

impl AppState {
    /// Builds application state using configured storage and providers.
    pub async fn initialize(settings: Settings) -> MemcoreResult<Self> {
        Self::initialize_with_shutdown(settings, ShutdownToken::new()).await
    }

    /// Builds application state using a caller-provided shutdown token.
    pub async fn initialize_with_shutdown(
        settings: Settings,
        shutdown_token: ShutdownToken,
    ) -> MemcoreResult<Self> {
        let storage_startup = run_storage_startup_checks(&settings).await?;
        let wiring = ProviderWiring::from_settings(&settings).await?;
        let org_plan_store = create_org_plan_store(&settings).await?;
        let memory_usage_snapshot_store = create_memory_usage_snapshot_store(&settings).await?;
        let background_job_run_store = create_background_job_run_store(&settings).await?;
        let background_job_lock_store = create_background_job_lock_store(&settings).await?;
        let background_job_lock_owner_id = settings
            .background_job_lock_enabled
            .then(|| background_job_lock_owner_id(&settings));
        let memory_engine = Arc::new(
            create_memory_engine(
                &settings,
                &wiring,
                org_plan_store.clone(),
                memory_usage_snapshot_store,
            )
            .await?,
        );
        let background_jobs = Arc::new(create_background_job_runner(
            &settings,
            memory_engine.clone(),
            background_job_run_store.clone(),
            background_job_lock_store.clone(),
            background_job_lock_owner_id.clone(),
            shutdown_token.child_token(),
        ));
        if settings.background_jobs_enabled {
            let runner = background_jobs.clone();
            tokio::spawn(async move {
                runner.run_forever().await;
            });
        }
        let api_key_store = create_api_key_store(&settings).await?;
        Ok(Self {
            settings: settings.clone(),
            started_at: Utc::now(),
            memory_engine: memory_engine.clone(),
            api_key_store,
            rate_limiter: create_rate_limiter(&settings),
            metrics: Arc::new(Metrics::default()),
            provider_usage: wiring.usage_recorder,
            provider_usage_store: wiring.usage_store,
            org_plan_store,
            background_jobs,
            background_job_run_store,
            background_job_lock_store,
            background_job_lock_owner_id,
            shutdown_token,
            storage_startup,
        })
    }

    /// Synchronous helper for tests when fact, event, and vector backends are all mock.
    ///
    /// For SQLite, Postgres, or LanceDB backends, call [`Self::initialize`] from async code instead.
    pub fn new(settings: Settings) -> Self {
        if settings.fact_backend == FactBackend::Mock
            && settings.event_backend == EventBackend::Mock
            && settings.vector_backend == VectorBackend::Mock
        {
            let wiring = ProviderWiring::for_mock_tests(&settings);
            let org_plan_store = Arc::new(MockOrgPlanStore::new());
            let memory_usage_snapshot_store = Arc::new(MockMemoryUsageSnapshotStore::new());
            let background_job_run_store = if settings.background_job_history_enabled {
                Some(Arc::new(MockBackgroundJobRunStore::new()) as Arc<dyn BackgroundJobRunStore>)
            } else {
                None
            };
            let background_job_lock_store = if settings.background_job_lock_enabled {
                Some(Arc::new(MockBackgroundJobLockStore::new()) as Arc<dyn BackgroundJobLockStore>)
            } else {
                None
            };
            let background_job_lock_owner_id = settings
                .background_job_lock_enabled
                .then(|| background_job_lock_owner_id(&settings));
            let memory_engine = Arc::new(
                create_mock_memory_engine_with_wiring_and_org_plan_store(
                    &settings,
                    &wiring,
                    org_plan_store.clone(),
                    memory_usage_snapshot_store,
                )
                .expect("failed to create mock memory engine"),
            );
            let background_jobs = Arc::new(create_background_job_runner(
                &settings,
                memory_engine.clone(),
                background_job_run_store.clone(),
                background_job_lock_store.clone(),
                background_job_lock_owner_id.clone(),
                ShutdownToken::new(),
            ));
            Self {
                started_at: Utc::now(),
                memory_engine,
                api_key_store: Arc::new(MockApiKeyStore::new()),
                settings: settings.clone(),
                rate_limiter: create_rate_limiter(&settings),
                metrics: Arc::new(Metrics::default()),
                provider_usage: wiring.usage_recorder,
                provider_usage_store: wiring.usage_store,
                org_plan_store,
                background_jobs,
                background_job_run_store,
                background_job_lock_store,
                background_job_lock_owner_id,
                shutdown_token: ShutdownToken::new(),
                storage_startup: StorageStartupCheckReport::ready_without_database(),
            }
        } else {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(Self::initialize(settings))
            })
            .expect("failed to initialize AppState")
        }
    }

    pub fn with_memory_engine(settings: Settings, memory_engine: Arc<MemoryEngine>) -> Self {
        Self {
            settings: settings.clone(),
            started_at: Utc::now(),
            memory_engine: memory_engine.clone(),
            api_key_store: Arc::new(MockApiKeyStore::new()),
            rate_limiter: create_rate_limiter(&settings),
            metrics: Arc::new(Metrics::default()),
            provider_usage: provider_usage_recorder(settings.provider_usage_metrics_enabled),
            provider_usage_store: None,
            org_plan_store: Arc::new(MockOrgPlanStore::new()),
            background_jobs: Arc::new(create_background_job_runner(
                &settings,
                memory_engine.clone(),
                None,
                None,
                None,
                ShutdownToken::new(),
            )),
            background_job_run_store: None,
            background_job_lock_store: None,
            background_job_lock_owner_id: None,
            shutdown_token: ShutdownToken::new(),
            storage_startup: StorageStartupCheckReport::ready_without_database(),
        }
    }

    pub fn with_memory_engine_and_provider_usage(
        settings: Settings,
        memory_engine: Arc<MemoryEngine>,
        provider_usage: Arc<dyn ProviderUsageRecorder>,
    ) -> Self {
        Self {
            settings: settings.clone(),
            started_at: Utc::now(),
            memory_engine: memory_engine.clone(),
            api_key_store: Arc::new(MockApiKeyStore::new()),
            rate_limiter: create_rate_limiter(&settings),
            metrics: Arc::new(Metrics::default()),
            provider_usage,
            provider_usage_store: None,
            org_plan_store: Arc::new(MockOrgPlanStore::new()),
            background_jobs: Arc::new(create_background_job_runner(
                &settings,
                memory_engine.clone(),
                None,
                None,
                None,
                ShutdownToken::new(),
            )),
            background_job_run_store: None,
            background_job_lock_store: None,
            background_job_lock_owner_id: None,
            shutdown_token: ShutdownToken::new(),
            storage_startup: StorageStartupCheckReport::ready_without_database(),
        }
    }

    pub fn with_memory_engine_provider_usage_and_store(
        settings: Settings,
        memory_engine: Arc<MemoryEngine>,
        provider_usage: Arc<dyn ProviderUsageRecorder>,
        provider_usage_store: Option<Arc<dyn ProviderUsageStore>>,
    ) -> Self {
        Self {
            settings: settings.clone(),
            started_at: Utc::now(),
            memory_engine: memory_engine.clone(),
            api_key_store: Arc::new(MockApiKeyStore::new()),
            rate_limiter: create_rate_limiter(&settings),
            metrics: Arc::new(Metrics::default()),
            provider_usage,
            provider_usage_store,
            org_plan_store: Arc::new(MockOrgPlanStore::new()),
            background_jobs: Arc::new(create_background_job_runner(
                &settings,
                memory_engine.clone(),
                None,
                None,
                None,
                ShutdownToken::new(),
            )),
            background_job_run_store: None,
            background_job_lock_store: None,
            background_job_lock_owner_id: None,
            shutdown_token: ShutdownToken::new(),
            storage_startup: StorageStartupCheckReport::ready_without_database(),
        }
    }
}

async fn run_storage_startup_checks(
    settings: &Settings,
) -> MemcoreResult<StorageStartupCheckReport> {
    let mode = storage_migration_mode(settings);
    let require_clean = settings.database_require_clean_migrations;
    let uses_postgres = settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres;
    let uses_sqlite = settings.fact_backend == FactBackend::Sqlite
        || settings.event_backend == EventBackend::Sqlite;

    tracing::info!(
        migration_mode = migration_mode_label(mode),
        require_clean_migrations = require_clean,
        uses_sqlite,
        uses_postgres,
        "running storage startup checks"
    );

    let postgres_report = if uses_postgres {
        #[cfg(feature = "postgres")]
        {
            let url = require_postgres_url(settings)?;
            Some(check_postgres_startup(&url, mode, require_clean).await?)
        }
        #[cfg(not(feature = "postgres"))]
        {
            if settings.event_backend == EventBackend::Postgres {
                return Err(MemcoreError::ValidationError(
                    "Postgres event backend requires the `postgres` cargo feature".to_string(),
                ));
            }
            return Err(MemcoreError::ValidationError(
                "Postgres fact backend requires the `postgres` cargo feature".to_string(),
            ));
        }
    } else {
        None
    };

    let sqlite_report = if uses_sqlite {
        Some(check_sqlite_startup(&settings.database_url, mode, require_clean).await?)
    } else {
        None
    };

    let report = combine_storage_startup_reports(postgres_report, sqlite_report);
    tracing::info!(
        database_connected = report.database_connected,
        migrations_clean = report.migrations_clean,
        pending_migrations = report
            .migration_report
            .as_ref()
            .map(|report| report.pending_count),
        warning_count = report.warnings.len(),
        "storage startup checks completed"
    );
    Ok(report)
}

fn combine_storage_startup_reports(
    first: Option<StorageStartupCheckReport>,
    second: Option<StorageStartupCheckReport>,
) -> StorageStartupCheckReport {
    match (first, second) {
        (None, None) => StorageStartupCheckReport::ready_without_database(),
        (Some(report), None) | (None, Some(report)) => report,
        (Some(first), Some(second)) => {
            let mut warnings = first.warnings;
            warnings.extend(second.warnings);
            StorageStartupCheckReport {
                database_connected: first.database_connected && second.database_connected,
                migrations_clean: first.migrations_clean && second.migrations_clean,
                migration_report: first.migration_report.or(second.migration_report),
                warnings,
            }
        }
    }
}

fn storage_migration_mode(settings: &Settings) -> StorageMigrationMode {
    if !settings.database_migrations_enabled {
        return StorageMigrationMode::Disabled;
    }

    match settings.database_migration_mode {
        DatabaseMigrationMode::Auto => StorageMigrationMode::Auto,
        DatabaseMigrationMode::ValidateOnly => StorageMigrationMode::ValidateOnly,
        DatabaseMigrationMode::Disabled => StorageMigrationMode::Disabled,
    }
}

fn migration_mode_label(mode: StorageMigrationMode) -> &'static str {
    match mode {
        StorageMigrationMode::Auto => "auto",
        StorageMigrationMode::ValidateOnly => "validate_only",
        StorageMigrationMode::Disabled => "disabled",
    }
}

fn is_sqlite_memory_database(settings: &Settings) -> bool {
    settings.database_url.contains(":memory:")
}

fn create_background_job_runner(
    settings: &Settings,
    memory_engine: Arc<MemoryEngine>,
    background_job_run_store: Option<Arc<dyn BackgroundJobRunStore>>,
    background_job_lock_store: Option<Arc<dyn BackgroundJobLockStore>>,
    background_job_lock_owner_id: Option<String>,
    shutdown_token: ShutdownToken,
) -> BackgroundJobRunner {
    let org_ids = settings.background_job_org_ids.clone();
    let jobs: Vec<Arc<dyn BackgroundJob>> = vec![
        Arc::new(MemoryUsageSnapshotJob::new(
            memory_engine.clone(),
            settings.memory_usage_snapshot_job_enabled,
            Duration::from_secs(settings.memory_usage_snapshot_job_interval_seconds),
            org_ids.clone(),
        )),
        Arc::new(ProviderUsageRetentionJob::new(
            memory_engine,
            settings.provider_usage_retention_job_enabled,
            Duration::from_secs(settings.provider_usage_retention_job_interval_seconds),
            org_ids,
            settings.provider_usage_retention_days,
        )),
        Arc::new(MemoryRetentionJob::new(
            settings.memory_retention_job_enabled,
            Duration::from_secs(settings.memory_retention_job_interval_seconds),
        )),
    ];

    BackgroundJobRunner::new(
        settings.background_jobs_enabled,
        Duration::from_secs(settings.background_job_runner_interval_seconds),
        jobs,
    )
    .with_history_store(
        settings.background_job_history_enabled,
        background_job_run_store,
    )
    .with_lock_store(
        settings.background_job_lock_enabled,
        background_job_lock_owner_id.unwrap_or_default(),
        Duration::from_secs(settings.background_job_lock_ttl_seconds),
        background_job_lock_store,
    )
    .with_retry_policy(background_job_retry_policy(settings))
    .with_shutdown(
        shutdown_token,
        Duration::from_secs(settings.background_job_shutdown_timeout_seconds),
    )
}

fn create_rate_limiter(settings: &Settings) -> Arc<RateLimiter> {
    Arc::new(RateLimiter::new(
        settings.rate_limit_enabled,
        settings.rate_limit_requests_per_minute,
    ))
}

fn org_quota_limits_from_settings(settings: &Settings) -> OrgQuotaLimits {
    OrgQuotaLimits::from_raw(
        settings.quotas_enabled,
        settings.max_users_per_org,
        settings.max_memories_per_user,
        settings.max_memories_per_org,
        settings.daily_provider_request_limit,
        settings.daily_provider_token_limit,
    )
}

/// Wires `MemoryEngine` from settings: configurable fact/vector stores and LLM/embedding providers.
pub async fn create_memory_engine(
    settings: &Settings,
    wiring: &ProviderWiring,
    org_plan_store: Arc<dyn OrgPlanStore>,
    memory_usage_snapshot_store: Arc<dyn MemoryUsageSnapshotStore>,
) -> MemcoreResult<MemoryEngine> {
    let (fact_store, event_store) = create_storage(settings).await?;
    let llm_provider = create_llm_provider(
        settings,
        wiring.usage_recorder.clone(),
        wiring.attribution_slot.clone(),
    )?;
    let embedding_provider = create_embedding_provider(
        settings,
        wiring.usage_recorder.clone(),
        wiring.attribution_slot.clone(),
    )?;
    let vector_store = create_vector_store(settings, embedding_provider.dimensions()).await?;

    Ok(apply_context_cache_async(
        MemoryEngine::new(fact_store, vector_store, llm_provider, embedding_provider)
            .with_pii_redaction(settings.enable_pii_redaction)
            .with_event_store(event_store)
            .with_usage_attribution(wiring.attribution_slot.clone())
            .with_provider_usage_store(wiring.usage_store.clone())
            .with_org_plan_store(org_plan_store)
            .with_memory_usage_snapshot_store(Some(memory_usage_snapshot_store))
            .with_global_quota_limits(org_quota_limits_from_settings(settings))
            .with_audit_provider_info(
                Some(llm_provider_name(settings)),
                Some(settings.llm_model.clone()),
            ),
        settings,
    )
    .await?)
}

async fn create_api_key_store(settings: &Settings) -> MemcoreResult<Arc<dyn ApiKeyStore>> {
    if settings.auth_mode == AuthMode::Dev {
        return Ok(Arc::new(MockApiKeyStore::new()));
    }

    #[cfg(feature = "postgres")]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        let postgres_url = require_postgres_url(settings)?;
        let store = PostgresApiKeyStore::new(connect_postgres_pool(&postgres_url).await?);
        return Ok(Arc::new(store));
    }

    #[cfg(not(feature = "postgres"))]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        return Err(MemcoreError::ValidationError(
            "database auth with postgres storage requires the `postgres` cargo feature".to_string(),
        ));
    }

    let store = if is_sqlite_memory_database(settings) {
        SqliteApiKeyStore::connect(&settings.database_url).await?
    } else {
        SqliteApiKeyStore::new(connect_sqlite_pool(&settings.database_url).await?)
    };
    Ok(Arc::new(store))
}

async fn create_org_plan_store(settings: &Settings) -> MemcoreResult<Arc<dyn OrgPlanStore>> {
    if settings.fact_backend == FactBackend::Mock && settings.event_backend == EventBackend::Mock {
        return Ok(Arc::new(MockOrgPlanStore::new()));
    }

    #[cfg(feature = "postgres")]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        let postgres_url = require_postgres_url(settings)?;
        let store = PostgresOrgPlanStore::new(connect_postgres_pool(&postgres_url).await?);
        return Ok(Arc::new(store));
    }

    #[cfg(not(feature = "postgres"))]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        if settings.event_backend == EventBackend::Postgres {
            return Err(MemcoreError::ValidationError(
                "Postgres event backend requires the `postgres` cargo feature".to_string(),
            ));
        }
        return Err(MemcoreError::ValidationError(
            "Postgres fact backend requires the `postgres` cargo feature".to_string(),
        ));
    }

    let store = if is_sqlite_memory_database(settings) {
        SqliteOrgPlanStore::connect(&settings.database_url).await?
    } else {
        SqliteOrgPlanStore::new(connect_sqlite_pool(&settings.database_url).await?)
    };
    Ok(Arc::new(store))
}

async fn create_memory_usage_snapshot_store(
    settings: &Settings,
) -> MemcoreResult<Arc<dyn MemoryUsageSnapshotStore>> {
    if settings.fact_backend == FactBackend::Mock && settings.event_backend == EventBackend::Mock {
        return Ok(Arc::new(MockMemoryUsageSnapshotStore::new()));
    }

    #[cfg(feature = "postgres")]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        let postgres_url = require_postgres_url(settings)?;
        let store =
            PostgresMemoryUsageSnapshotStore::new(connect_postgres_pool(&postgres_url).await?);
        return Ok(Arc::new(store));
    }

    #[cfg(not(feature = "postgres"))]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        return Err(MemcoreError::ValidationError(
            "Postgres storage requires the `postgres` cargo feature".to_string(),
        ));
    }

    let store = if is_sqlite_memory_database(settings) {
        SqliteMemoryUsageSnapshotStore::connect(&settings.database_url).await?
    } else {
        SqliteMemoryUsageSnapshotStore::new(connect_sqlite_pool(&settings.database_url).await?)
    };
    Ok(Arc::new(store))
}

async fn create_storage(
    settings: &Settings,
) -> MemcoreResult<(Arc<dyn FactStore>, Arc<dyn MemoryEventStore>)> {
    #[cfg(not(feature = "postgres"))]
    if settings.fact_backend == FactBackend::Postgres
        || settings.event_backend == EventBackend::Postgres
    {
        if settings.event_backend == EventBackend::Postgres {
            return Err(MemcoreError::ValidationError(
                "Postgres event backend requires the `postgres` cargo feature".to_string(),
            ));
        }
        return Err(MemcoreError::ValidationError(
            "Postgres fact backend requires the `postgres` cargo feature".to_string(),
        ));
    }

    match (&settings.fact_backend, &settings.event_backend) {
        (FactBackend::Mock, EventBackend::Mock) => Ok((
            Arc::new(MockFactStore::new()),
            Arc::new(MockMemoryEventStore::new()),
        )),
        (FactBackend::Sqlite, EventBackend::Sqlite) => {
            let fact_store = if is_sqlite_memory_database(settings) {
                SqliteFactStore::connect(&settings.database_url).await?
            } else {
                SqliteFactStore::new(connect_sqlite_pool(&settings.database_url).await?)
            };
            let event_store = SqliteMemoryEventStore::new(fact_store.pool());
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        (FactBackend::Sqlite, EventBackend::Mock) => {
            let fact_store = if is_sqlite_memory_database(settings) {
                SqliteFactStore::connect(&settings.database_url).await?
            } else {
                SqliteFactStore::new(connect_sqlite_pool(&settings.database_url).await?)
            };
            Ok((Arc::new(fact_store), Arc::new(MockMemoryEventStore::new())))
        }
        (FactBackend::Mock, EventBackend::Sqlite) => {
            let event_store = if is_sqlite_memory_database(settings) {
                SqliteMemoryEventStore::connect(&settings.database_url).await?
            } else {
                SqliteMemoryEventStore::new(connect_sqlite_pool(&settings.database_url).await?)
            };
            Ok((Arc::new(MockFactStore::new()), Arc::new(event_store)))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Postgres, EventBackend::Postgres) => {
            let postgres_url = require_postgres_url(settings)?;
            let fact_store = PostgresFactStore::new(connect_postgres_pool(&postgres_url).await?);
            let event_store = PostgresMemoryEventStore::new(fact_store.pool());
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Postgres, EventBackend::Mock) => {
            let postgres_url = require_postgres_url(settings)?;
            let fact_store = PostgresFactStore::new(connect_postgres_pool(&postgres_url).await?);
            Ok((Arc::new(fact_store), Arc::new(MockMemoryEventStore::new())))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Mock, EventBackend::Postgres) => {
            let postgres_url = require_postgres_url(settings)?;
            let event_store =
                PostgresMemoryEventStore::new(connect_postgres_pool(&postgres_url).await?);
            Ok((Arc::new(MockFactStore::new()), Arc::new(event_store)))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Sqlite, EventBackend::Postgres) => {
            let fact_store = if is_sqlite_memory_database(settings) {
                SqliteFactStore::connect(&settings.database_url).await?
            } else {
                SqliteFactStore::new(connect_sqlite_pool(&settings.database_url).await?)
            };
            let postgres_url = require_postgres_url(settings)?;
            let event_store =
                PostgresMemoryEventStore::new(connect_postgres_pool(&postgres_url).await?);
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        #[cfg(feature = "postgres")]
        (FactBackend::Postgres, EventBackend::Sqlite) => {
            let postgres_url = require_postgres_url(settings)?;
            let fact_store = PostgresFactStore::new(connect_postgres_pool(&postgres_url).await?);
            let event_store = if is_sqlite_memory_database(settings) {
                SqliteMemoryEventStore::connect(&settings.database_url).await?
            } else {
                SqliteMemoryEventStore::new(connect_sqlite_pool(&settings.database_url).await?)
            };
            Ok((Arc::new(fact_store), Arc::new(event_store)))
        }
        #[cfg(not(feature = "postgres"))]
        (FactBackend::Postgres, _) | (_, EventBackend::Postgres) => {
            Err(MemcoreError::ValidationError(
                "Postgres storage requires the `postgres` cargo feature".to_string(),
            ))
        }
    }
}

#[cfg(feature = "postgres")]
fn require_postgres_url(settings: &Settings) -> MemcoreResult<String> {
    settings
        .postgres_url
        .as_ref()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            if settings.event_backend == EventBackend::Postgres
                && settings.fact_backend != FactBackend::Postgres
            {
                MemcoreError::ValidationError(
                    "MEMCORE_POSTGRES_URL is required when MEMCORE_EVENT_BACKEND=postgres"
                        .to_string(),
                )
            } else {
                MemcoreError::ValidationError(
                    "MEMCORE_POSTGRES_URL is required when MEMCORE_FACT_BACKEND=postgres"
                        .to_string(),
                )
            }
        })
}

fn provider_execution_policy(settings: &Settings) -> MemcoreResult<ProviderExecutionPolicy> {
    ProviderExecutionPolicy::from_config(
        settings.provider_timeout_seconds,
        settings.provider_max_retries,
        settings.provider_initial_backoff_ms,
        settings.provider_max_backoff_ms,
        settings.provider_backoff_multiplier,
        settings.provider_retry_jitter_enabled,
    )
}

fn background_job_retry_policy(settings: &Settings) -> BackgroundJobRetryPolicy {
    BackgroundJobRetryPolicy {
        enabled: settings.background_job_retries_enabled,
        max_retries: settings.background_job_max_retries,
        initial_backoff: Duration::from_millis(settings.background_job_initial_backoff_ms),
        max_backoff: Duration::from_millis(settings.background_job_max_backoff_ms),
        backoff_multiplier: settings.background_job_backoff_multiplier,
        jitter_enabled: settings.background_job_retry_jitter_enabled,
    }
}

fn create_llm_provider(
    settings: &Settings,
    usage_recorder: Arc<dyn ProviderUsageRecorder>,
    attribution_slot: Arc<ProviderUsageAttributionSlot>,
) -> MemcoreResult<Arc<dyn LlmProvider>> {
    let runtime = provider_runtime(settings)?;
    let llm_names = if settings.provider_fallback_enabled {
        settings.llm_fallback_order.clone()
    } else {
        vec![llm_provider_kind_name(&settings.llm_provider).to_string()]
    };
    let summarizer_names = if settings.provider_fallback_enabled {
        settings.summarizer_fallback_order.clone()
    } else {
        vec![llm_provider_kind_name(&settings.llm_provider).to_string()]
    };

    if settings.provider_fallback_enabled {
        validate_provider_fallback_order(&llm_names, validate_llm_provider_name)?;
        validate_provider_fallback_order(&summarizer_names, validate_summarizer_provider_name)?;
    } else if matches!(
        settings.llm_provider,
        LlmProviderKind::OpenRouter | LlmProviderKind::Anthropic | LlmProviderKind::Groq
    ) {
        return build_llm_provider_by_name(
            llm_provider_kind_name(&settings.llm_provider),
            settings,
        );
    }

    let providers = llm_candidates_from_names(&llm_names, ProviderCapability::Llm, settings)?;
    let summarizer_providers = llm_candidates_from_names(
        &summarizer_names,
        ProviderCapability::Summarization,
        settings,
    )?;

    Ok(build_resilient_llm_from_candidates(
        providers,
        summarizer_providers,
        runtime.circuit_breaker,
        runtime.policy,
        settings.provider_fallback_enabled,
        Some(runtime.metrics),
        Some(usage_recorder),
        Some(attribution_slot),
        settings.provider_cost_tracking_enabled,
    ))
}

fn create_embedding_provider(
    settings: &Settings,
    usage_recorder: Arc<dyn ProviderUsageRecorder>,
    attribution_slot: Arc<ProviderUsageAttributionSlot>,
) -> MemcoreResult<Arc<dyn EmbeddingProvider>> {
    let runtime = provider_runtime(settings)?;
    let names = if settings.provider_fallback_enabled {
        settings.embedding_fallback_order.clone()
    } else {
        vec![embedding_provider_kind_name(&settings.embedding_provider).to_string()]
    };

    if settings.provider_fallback_enabled {
        validate_provider_fallback_order(&names, validate_embedding_provider_name)?;
    }

    let providers = embedding_candidates_from_names(&names, settings)?;

    build_resilient_embedding_from_candidates(
        providers,
        runtime.circuit_breaker,
        runtime.policy,
        settings.provider_fallback_enabled,
        Some(runtime.metrics),
        Some(usage_recorder),
        Some(attribution_slot),
        settings.provider_cost_tracking_enabled,
    )
}

fn require_openai_api_key(settings: &Settings, message: &str) -> MemcoreResult<String> {
    settings
        .openai_api_key
        .as_ref()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .ok_or_else(|| MemcoreError::ValidationError(message.to_string()))
}

fn provider_init_error(provider: &str, err: MemcoreError) -> MemcoreError {
    MemcoreError::ValidationError(format!("failed to initialize {provider} provider: {err}"))
}

async fn create_vector_store(
    settings: &Settings,
    dimensions: usize,
) -> MemcoreResult<Arc<dyn VectorStore>> {
    match settings.vector_backend {
        VectorBackend::Mock => Ok(Arc::new(MockVectorStore::new())),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                let store = LanceDbVectorStore::new_or_open(
                    &settings.lancedb_path,
                    &settings.lancedb_table,
                    dimensions,
                )
                .await?;
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "lancedb"))]
            {
                let _ = dimensions;
                Err(MemcoreError::ValidationError(
                    "LanceDB vector backend requires the `lancedb` cargo feature".to_string(),
                ))
            }
        }
        VectorBackend::Qdrant => {
            #[cfg(feature = "qdrant")]
            {
                let url = require_qdrant_url(settings)?;
                let store =
                    QdrantVectorStore::connect(&url, &settings.qdrant_collection, dimensions)
                        .await?;
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "qdrant"))]
            {
                let _ = dimensions;
                Err(MemcoreError::ValidationError(
                    "Qdrant vector backend requires the `qdrant` cargo feature".to_string(),
                ))
            }
        }
    }
}

#[cfg(feature = "qdrant")]
fn require_qdrant_url(settings: &Settings) -> MemcoreResult<String> {
    let url = settings.qdrant_url.trim();
    if url.is_empty() {
        return Err(MemcoreError::ValidationError(
            "MEMCORE_QDRANT_URL is required when MEMCORE_VECTOR_BACKEND=qdrant".to_string(),
        ));
    }
    Ok(url.to_string())
}

/// In-memory mock fact and vector stores for fast API tests.
pub fn create_mock_memory_engine(settings: &Settings) -> MemcoreResult<MemoryEngine> {
    create_mock_memory_engine_with_wiring(settings, &ProviderWiring::for_mock_tests(settings))
}

pub fn create_mock_memory_engine_with_usage(
    settings: &Settings,
    usage_recorder: Arc<dyn ProviderUsageRecorder>,
) -> MemcoreResult<MemoryEngine> {
    create_mock_memory_engine_with_wiring(settings, &ProviderWiring::for_tests(usage_recorder))
}

pub fn create_mock_memory_engine_with_wiring(
    settings: &Settings,
    wiring: &ProviderWiring,
) -> MemcoreResult<MemoryEngine> {
    create_mock_memory_engine_with_wiring_and_org_plan_store(
        settings,
        wiring,
        Arc::new(MockOrgPlanStore::new()),
        Arc::new(MockMemoryUsageSnapshotStore::new()),
    )
}

pub fn create_mock_memory_engine_with_wiring_and_org_plan_store(
    settings: &Settings,
    wiring: &ProviderWiring,
    org_plan_store: Arc<dyn OrgPlanStore>,
    memory_usage_snapshot_store: Arc<dyn MemoryUsageSnapshotStore>,
) -> MemcoreResult<MemoryEngine> {
    apply_context_cache(
        MemoryEngine::new(
            Arc::new(MockFactStore::new()),
            Arc::new(MockVectorStore::new()),
            create_llm_provider(
                settings,
                wiring.usage_recorder.clone(),
                wiring.attribution_slot.clone(),
            )?,
            create_embedding_provider(
                settings,
                wiring.usage_recorder.clone(),
                wiring.attribution_slot.clone(),
            )?,
        )
        .with_pii_redaction(settings.enable_pii_redaction)
        .with_event_store(Arc::new(MockMemoryEventStore::new()))
        .with_usage_attribution(wiring.attribution_slot.clone())
        .with_provider_usage_store(wiring.usage_store.clone())
        .with_org_plan_store(org_plan_store)
        .with_memory_usage_snapshot_store(Some(memory_usage_snapshot_store))
        .with_global_quota_limits(org_quota_limits_from_settings(settings))
        .with_audit_provider_info(
            Some(llm_provider_name(settings)),
            Some(settings.llm_model.clone()),
        ),
        settings,
    )
}

fn context_cache_config_from_settings(settings: &Settings) -> ContextCacheConfig {
    ContextCacheConfig {
        enabled: settings.context_cache_enabled,
        ttl_seconds: settings.context_cache_ttl_seconds,
        max_entries: settings.context_cache_max_entries,
        stampede_protection_enabled: settings.context_cache_stampede_protection_enabled,
        stampede_lock_timeout_seconds: settings.context_cache_lock_timeout_seconds,
        stale_while_revalidate_enabled: settings.context_cache_stale_while_revalidate_enabled,
        stale_ttl_seconds: settings.context_cache_stale_ttl_seconds,
        metrics_enabled: settings.context_cache_metrics_enabled,
    }
}

fn apply_context_cache(engine: MemoryEngine, settings: &Settings) -> MemcoreResult<MemoryEngine> {
    if settings.context_cache_backend == ContextCacheBackend::Redis {
        return Err(MemcoreError::ValidationError(
            "Redis context cache backend requires async AppState::initialize".to_string(),
        ));
    }

    let config = context_cache_config_from_settings(settings);
    config.validate()?;

    Ok(engine.with_context_cache(
        Arc::new(InMemoryContextCache::new(
            settings.context_cache_max_entries,
        )),
        config,
    ))
}

async fn apply_context_cache_async(
    engine: MemoryEngine,
    settings: &Settings,
) -> MemcoreResult<MemoryEngine> {
    if settings.context_cache_backend != ContextCacheBackend::Redis {
        return apply_context_cache(engine, settings);
    }

    let config = context_cache_config_from_settings(settings);
    config.validate()?;

    #[cfg(feature = "redis-cache")]
    {
        let redis_url = require_redis_url(settings)?;
        let cache = RedisContextCache::connect(
            &redis_url,
            settings.context_cache_key_prefix.clone(),
            settings.context_cache_ttl_seconds,
        )
        .await?;
        return Ok(engine.with_context_cache(Arc::new(cache), config));
    }

    #[cfg(not(feature = "redis-cache"))]
    {
        Err(MemcoreError::ValidationError(
            "Redis context cache backend requires the `redis-cache` cargo feature".to_string(),
        ))
    }
}

#[cfg(feature = "redis-cache")]
fn require_redis_url(settings: &Settings) -> MemcoreResult<String> {
    settings
        .redis_url
        .as_ref()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            MemcoreError::ValidationError(
                "MEMCORE_REDIS_URL is required when MEMCORE_CONTEXT_CACHE_BACKEND=redis"
                    .to_string(),
            )
        })
}

fn llm_provider_name(settings: &Settings) -> String {
    match settings.llm_provider {
        LlmProviderKind::Mock => "mock".to_string(),
        LlmProviderKind::OpenAi => "openai".to_string(),
        LlmProviderKind::OpenRouter => "openrouter".to_string(),
        LlmProviderKind::Anthropic => "anthropic".to_string(),
        LlmProviderKind::Groq => "groq".to_string(),
    }
}
