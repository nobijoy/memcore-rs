use std::env;
use std::str::FromStr;

use memcore_common::{MemcoreError, MemcoreResult};

const MEMCORE_ENV: &str = "MEMCORE_ENV";
const MEMCORE_HOST: &str = "MEMCORE_HOST";
const MEMCORE_PORT: &str = "MEMCORE_PORT";
const MEMCORE_STORAGE_MODE: &str = "MEMCORE_STORAGE_MODE";
const MEMCORE_VECTOR_BACKEND: &str = "MEMCORE_VECTOR_BACKEND";
const MEMCORE_FACT_BACKEND: &str = "MEMCORE_FACT_BACKEND";
const MEMCORE_EVENT_BACKEND: &str = "MEMCORE_EVENT_BACKEND";
const MEMCORE_DATABASE_URL: &str = "MEMCORE_DATABASE_URL";
const MEMCORE_POSTGRES_URL: &str = "MEMCORE_POSTGRES_URL";
const MEMCORE_QDRANT_URL: &str = "MEMCORE_QDRANT_URL";
const MEMCORE_QDRANT_COLLECTION: &str = "MEMCORE_QDRANT_COLLECTION";
const MEMCORE_LANCEDB_PATH: &str = "MEMCORE_LANCEDB_PATH";
const MEMCORE_LANCEDB_TABLE: &str = "MEMCORE_LANCEDB_TABLE";
const MEMCORE_LLM_PROVIDER: &str = "MEMCORE_LLM_PROVIDER";
const MEMCORE_LLM_MODEL: &str = "MEMCORE_LLM_MODEL";
const MEMCORE_EMBEDDING_PROVIDER: &str = "MEMCORE_EMBEDDING_PROVIDER";
const MEMCORE_EMBEDDING_MODEL: &str = "MEMCORE_EMBEDDING_MODEL";
const MEMCORE_ENABLE_PII_REDACTION: &str = "MEMCORE_ENABLE_PII_REDACTION";
const MEMCORE_MIN_IMPORTANCE: &str = "MEMCORE_MIN_IMPORTANCE";
const MEMCORE_AUTH_ENABLED: &str = "MEMCORE_AUTH_ENABLED";
const MEMCORE_AUTH_MODE: &str = "MEMCORE_AUTH_MODE";
const MEMCORE_DEV_API_KEY: &str = "MEMCORE_DEV_API_KEY";
const MEMCORE_API_KEY_PEPPER: &str = "MEMCORE_API_KEY_PEPPER";
const MEMCORE_RATE_LIMIT_ENABLED: &str = "MEMCORE_RATE_LIMIT_ENABLED";
const MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE: &str = "MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE";
const MEMCORE_LOG_FORMAT: &str = "MEMCORE_LOG_FORMAT";
const MEMCORE_LOG_LEVEL: &str = "MEMCORE_LOG_LEVEL";
const MEMCORE_REQUEST_ID_HEADER: &str = "MEMCORE_REQUEST_ID_HEADER";
const MEMCORE_METRICS_ENABLED: &str = "MEMCORE_METRICS_ENABLED";
const MEMCORE_RETENTION_ENABLED: &str = "MEMCORE_RETENTION_ENABLED";
const MEMCORE_FACT_RETENTION_DAYS: &str = "MEMCORE_FACT_RETENTION_DAYS";
const MEMCORE_EVENT_RETENTION_DAYS: &str = "MEMCORE_EVENT_RETENTION_DAYS";
const MEMCORE_CONTEXT_CACHE_ENABLED: &str = "MEMCORE_CONTEXT_CACHE_ENABLED";
const MEMCORE_CONTEXT_CACHE_BACKEND: &str = "MEMCORE_CONTEXT_CACHE_BACKEND";
const MEMCORE_CONTEXT_CACHE_TTL_SECONDS: &str = "MEMCORE_CONTEXT_CACHE_TTL_SECONDS";
const MEMCORE_CONTEXT_CACHE_MAX_ENTRIES: &str = "MEMCORE_CONTEXT_CACHE_MAX_ENTRIES";
const MEMCORE_CONTEXT_CACHE_KEY_PREFIX: &str = "MEMCORE_CONTEXT_CACHE_KEY_PREFIX";
const MEMCORE_REDIS_URL: &str = "MEMCORE_REDIS_URL";
const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const OPENAI_BASE_URL: &str = "OPENAI_BASE_URL";

pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_REQUEST_ID_HEADER: &str = "X-Request-ID";
pub const DEFAULT_CONTEXT_CACHE_KEY_PREFIX: &str = "memcore";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCacheBackend {
    Disabled,
    Memory,
    Redis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Dev,
    Database,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_filter_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Environment {
    Development,
    Production,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageMode {
    Embedded,
    Production,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorBackend {
    Mock,
    LanceDb,
    Qdrant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactBackend {
    Mock,
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventBackend {
    Mock,
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmProviderKind {
    Mock,
    OpenAi,
    OpenRouter,
    Anthropic,
    Groq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingProviderKind {
    Mock,
    OpenAi,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub environment: Environment,
    pub host: String,
    pub port: u16,
    pub storage_mode: StorageMode,
    pub vector_backend: VectorBackend,
    pub fact_backend: FactBackend,
    pub event_backend: EventBackend,
    pub database_url: String,
    pub postgres_url: Option<String>,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub lancedb_path: String,
    pub lancedb_table: String,
    pub llm_provider: LlmProviderKind,
    pub llm_model: String,
    pub embedding_provider: EmbeddingProviderKind,
    pub embedding_model: String,
    pub enable_pii_redaction: bool,
    pub min_importance: f32,
    /// Temporary development auth toggle.
    pub auth_enabled: bool,
    /// Authentication mode: dev API key or database-backed hashed keys.
    pub auth_mode: AuthMode,
    /// Temporary plaintext dev API key. Do not log this value.
    pub dev_api_key: String,
    /// Pepper for HMAC hashing of database-backed API keys. Required in database auth mode.
    pub api_key_pepper: Option<String>,
    /// OpenAI API key. Required only when LLM or embedding provider is OpenAI.
    pub openai_api_key: Option<String>,
    /// OpenAI API base URL (supports OpenAI-compatible gateways).
    pub openai_base_url: String,
    /// In-memory rate limiting toggle for protected API routes.
    pub rate_limit_enabled: bool,
    /// Maximum protected-route requests per organization per minute.
    pub rate_limit_requests_per_minute: u32,
    /// Structured log output format.
    pub log_format: LogFormat,
    /// Minimum tracing log level.
    pub log_level: LogLevel,
    /// HTTP header used for request correlation IDs.
    pub request_id_header: String,
    /// Expose in-process metrics at `GET /metrics`.
    pub metrics_enabled: bool,
    /// Global retention toggle for user-scoped retention apply endpoint.
    pub retention_enabled: bool,
    /// Default fact retention window in days (`0` disables fact retention).
    pub fact_retention_days: u32,
    /// Default event retention window in days (`0` disables event retention).
    pub event_retention_days: u32,
    /// In-memory context response cache toggle (derived from `context_cache_backend`).
    pub context_cache_enabled: bool,
    /// Context cache storage backend.
    pub context_cache_backend: ContextCacheBackend,
    /// Context cache entry TTL in seconds.
    pub context_cache_ttl_seconds: u64,
    /// Maximum in-memory context cache entries per process.
    pub context_cache_max_entries: usize,
    /// Redis URL for context cache when backend is `redis`.
    pub redis_url: Option<String>,
    /// Namespace prefix for Redis context cache keys.
    pub context_cache_key_prefix: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            environment: Environment::Development,
            host: "0.0.0.0".to_string(),
            port: 8080,
            storage_mode: StorageMode::Embedded,
            vector_backend: VectorBackend::Mock,
            fact_backend: FactBackend::Mock,
            event_backend: EventBackend::Mock,
            database_url: "sqlite://./data/memcore.db".to_string(),
            postgres_url: None,
            qdrant_url: "http://localhost:6333".to_string(),
            qdrant_collection: "memcore_vectors".to_string(),
            lancedb_path: "./data/lancedb".to_string(),
            lancedb_table: "memcore_vectors".to_string(),
            llm_provider: LlmProviderKind::Mock,
            llm_model: "mock-llm".to_string(),
            embedding_provider: EmbeddingProviderKind::Mock,
            embedding_model: "mock-embedding".to_string(),
            enable_pii_redaction: true,
            min_importance: 0.55,
            auth_enabled: true,
            auth_mode: AuthMode::Dev,
            dev_api_key: "memcore_dev_key".to_string(),
            api_key_pepper: None,
            openai_api_key: None,
            openai_base_url: DEFAULT_OPENAI_BASE_URL.to_string(),
            rate_limit_enabled: true,
            rate_limit_requests_per_minute: 60,
            log_format: LogFormat::Json,
            log_level: LogLevel::Info,
            request_id_header: DEFAULT_REQUEST_ID_HEADER.to_string(),
            metrics_enabled: true,
            retention_enabled: false,
            fact_retention_days: 0,
            event_retention_days: 0,
            context_cache_enabled: false,
            context_cache_backend: ContextCacheBackend::Disabled,
            context_cache_ttl_seconds: 300,
            context_cache_max_entries: 1000,
            redis_url: None,
            context_cache_key_prefix: DEFAULT_CONTEXT_CACHE_KEY_PREFIX.to_string(),
        }
    }
}

impl Settings {
    pub fn from_env() -> MemcoreResult<Self> {
        let defaults = Self::default();

        let environment = Environment::from_str(&read_env_or(MEMCORE_ENV, "development"))?;
        let host = read_env_or(MEMCORE_HOST, &defaults.host);
        let port = parse_u16(MEMCORE_PORT, defaults.port)?;
        let storage_mode =
            StorageMode::from_str(&read_env_or(MEMCORE_STORAGE_MODE, "embedded"))?;
        let vector_backend =
            VectorBackend::from_str(&read_env_or(MEMCORE_VECTOR_BACKEND, "lancedb"))?;
        let fact_backend = FactBackend::from_str(&read_env_or(MEMCORE_FACT_BACKEND, "sqlite"))?;
        let event_backend = match read_env_optional(MEMCORE_EVENT_BACKEND) {
            Some(value) => EventBackend::from_str(&value)?,
            None => EventBackend::default_for_fact_backend(&fact_backend),
        };
        let database_url = read_env_or(MEMCORE_DATABASE_URL, &defaults.database_url);
        let postgres_url = read_env_optional(MEMCORE_POSTGRES_URL);
        let qdrant_url = read_env_or(MEMCORE_QDRANT_URL, &defaults.qdrant_url);
        let qdrant_collection =
            read_env_or(MEMCORE_QDRANT_COLLECTION, &defaults.qdrant_collection);
        let lancedb_path = read_env_or(MEMCORE_LANCEDB_PATH, &defaults.lancedb_path);
        let lancedb_table = read_env_or(MEMCORE_LANCEDB_TABLE, &defaults.lancedb_table);
        let llm_provider =
            LlmProviderKind::from_str(&read_env_or(MEMCORE_LLM_PROVIDER, "mock"))?;
        let llm_model = read_env_or(MEMCORE_LLM_MODEL, &defaults.llm_model);
        let embedding_provider = EmbeddingProviderKind::from_str(&read_env_or(
            MEMCORE_EMBEDDING_PROVIDER,
            "mock",
        ))?;
        let embedding_model = read_env_or(MEMCORE_EMBEDDING_MODEL, &defaults.embedding_model);
        let enable_pii_redaction =
            parse_bool(MEMCORE_ENABLE_PII_REDACTION, defaults.enable_pii_redaction)?;
        let min_importance = parse_f32(MEMCORE_MIN_IMPORTANCE, defaults.min_importance)?;
        let auth_enabled = parse_bool(MEMCORE_AUTH_ENABLED, defaults.auth_enabled)?;
        let auth_mode = AuthMode::from_str(&read_env_or(MEMCORE_AUTH_MODE, "dev"))?;
        let dev_api_key = read_env_or(MEMCORE_DEV_API_KEY, &defaults.dev_api_key);
        let api_key_pepper = read_env_optional(MEMCORE_API_KEY_PEPPER);
        let openai_api_key = read_env_optional(OPENAI_API_KEY);
        let openai_base_url = read_env_or(OPENAI_BASE_URL, &defaults.openai_base_url);
        let rate_limit_enabled =
            parse_bool(MEMCORE_RATE_LIMIT_ENABLED, defaults.rate_limit_enabled)?;
        let rate_limit_requests_per_minute = parse_u32(
            MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE,
            defaults.rate_limit_requests_per_minute,
        )?;
        let log_format = LogFormat::from_str(&read_env_or(MEMCORE_LOG_FORMAT, "json"))?;
        let log_level = LogLevel::from_str(&read_env_or(MEMCORE_LOG_LEVEL, "info"))?;
        let request_id_header =
            read_env_or(MEMCORE_REQUEST_ID_HEADER, &defaults.request_id_header);
        let metrics_enabled = parse_bool(MEMCORE_METRICS_ENABLED, defaults.metrics_enabled)?;
        let retention_enabled =
            parse_bool(MEMCORE_RETENTION_ENABLED, defaults.retention_enabled)?;
        let fact_retention_days =
            parse_u32(MEMCORE_FACT_RETENTION_DAYS, defaults.fact_retention_days)?;
        let event_retention_days =
            parse_u32(MEMCORE_EVENT_RETENTION_DAYS, defaults.event_retention_days)?;
        let context_cache_backend = match read_env_optional(MEMCORE_CONTEXT_CACHE_BACKEND) {
            Some(value) => ContextCacheBackend::from_str(&value)?,
            None => {
                if parse_bool(MEMCORE_CONTEXT_CACHE_ENABLED, defaults.context_cache_enabled)? {
                    ContextCacheBackend::Memory
                } else {
                    ContextCacheBackend::Disabled
                }
            }
        };
        let context_cache_enabled = context_cache_backend != ContextCacheBackend::Disabled;
        let context_cache_ttl_seconds = parse_u64(
            MEMCORE_CONTEXT_CACHE_TTL_SECONDS,
            defaults.context_cache_ttl_seconds,
        )?;
        let context_cache_max_entries = parse_usize(
            MEMCORE_CONTEXT_CACHE_MAX_ENTRIES,
            defaults.context_cache_max_entries,
        )?;
        let redis_url = read_env_optional(MEMCORE_REDIS_URL);
        let context_cache_key_prefix = read_env_or(
            MEMCORE_CONTEXT_CACHE_KEY_PREFIX,
            &defaults.context_cache_key_prefix,
        );

        if !(0.0..=1.0).contains(&min_importance) {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_MIN_IMPORTANCE must be between 0.0 and 1.0".to_string(),
            ));
        }

        let settings = Self {
            environment,
            host,
            port,
            storage_mode,
            vector_backend,
            fact_backend,
            event_backend,
            database_url,
            postgres_url,
            qdrant_url,
            qdrant_collection,
            lancedb_path,
            lancedb_table,
            llm_provider,
            llm_model,
            embedding_provider,
            embedding_model,
            enable_pii_redaction,
            min_importance,
            auth_enabled,
            auth_mode,
            dev_api_key,
            api_key_pepper,
            openai_api_key,
            openai_base_url,
            rate_limit_enabled,
            rate_limit_requests_per_minute,
            log_format,
            log_level,
            request_id_header,
            metrics_enabled,
            retention_enabled,
            fact_retention_days,
            event_retention_days,
            context_cache_enabled,
            context_cache_backend,
            context_cache_ttl_seconds,
            context_cache_max_entries,
            redis_url,
            context_cache_key_prefix,
        };

        settings.validate()?;
        Ok(settings)
    }

    /// In-memory SQLite settings for integration tests.
    pub fn sqlite_memory() -> Self {
        Self {
            fact_backend: FactBackend::Sqlite,
            event_backend: EventBackend::Sqlite,
            database_url: "sqlite::memory:?cache=shared".to_string(),
            ..Self::default()
        }
    }

    /// LanceDB vector store with mock facts (API integration tests).
    pub fn lancedb_with_path(lancedb_path: impl Into<String>) -> Self {
        Self {
            vector_backend: VectorBackend::LanceDb,
            lancedb_path: lancedb_path.into(),
            ..Self::default()
        }
    }

    /// Qdrant vector store with mock facts (API integration tests).
    pub fn qdrant_with_url(qdrant_url: impl Into<String>) -> Self {
        Self {
            vector_backend: VectorBackend::Qdrant,
            qdrant_url: qdrant_url.into(),
            ..Self::default()
        }
    }

    /// Qdrant vector store with explicit collection name (API integration tests).
    pub fn qdrant_with_collection(
        qdrant_url: impl Into<String>,
        qdrant_collection: impl Into<String>,
    ) -> Self {
        Self {
            vector_backend: VectorBackend::Qdrant,
            qdrant_url: qdrant_url.into(),
            qdrant_collection: qdrant_collection.into(),
            ..Self::default()
        }
    }

    fn validate(&self) -> MemcoreResult<()> {
        if self.host.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_HOST cannot be empty".to_string(),
            ));
        }

        if self.database_url.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_DATABASE_URL cannot be empty".to_string(),
            ));
        }

        if self.lancedb_path.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_LANCEDB_PATH cannot be empty".to_string(),
            ));
        }

        if self.lancedb_table.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_LANCEDB_TABLE cannot be empty".to_string(),
            ));
        }

        if self.auth_enabled
            && self.auth_mode == AuthMode::Dev
            && self.dev_api_key.trim().is_empty()
        {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_DEV_API_KEY cannot be empty when MEMCORE_AUTH_ENABLED=true and MEMCORE_AUTH_MODE=dev"
                    .to_string(),
            ));
        }

        if self.auth_mode == AuthMode::Database
            && self
                .api_key_pepper
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_API_KEY_PEPPER is required when MEMCORE_AUTH_MODE=database".to_string(),
            ));
        }

        if self.openai_base_url.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "OPENAI_BASE_URL cannot be empty".to_string(),
            ));
        }

        if self.rate_limit_requests_per_minute == 0 {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE must be greater than 0".to_string(),
            ));
        }

        if self.request_id_header.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_REQUEST_ID_HEADER cannot be empty".to_string(),
            ));
        }

        let needs_openai_key = self.llm_provider == LlmProviderKind::OpenAi
            || self.embedding_provider == EmbeddingProviderKind::OpenAi;
        if needs_openai_key
            && self
                .openai_api_key
                .as_ref()
                .map(|k| k.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(MemcoreError::ValidationError(
                "OPENAI_API_KEY is required when MEMCORE_LLM_PROVIDER or MEMCORE_EMBEDDING_PROVIDER is openai"
                    .to_string(),
            ));
        }

        if self.fact_backend == FactBackend::Postgres
            && self
                .postgres_url
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_POSTGRES_URL is required when MEMCORE_FACT_BACKEND=postgres".to_string(),
            ));
        }

        if self.event_backend == EventBackend::Postgres
            && self
                .postgres_url
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_POSTGRES_URL is required when MEMCORE_EVENT_BACKEND=postgres".to_string(),
            ));
        }

        if self.fact_retention_days > 365_000 {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_FACT_RETENTION_DAYS is unreasonably large".to_string(),
            ));
        }

        if self.event_retention_days > 365_000 {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_EVENT_RETENTION_DAYS is unreasonably large".to_string(),
            ));
        }

        match self.context_cache_backend {
            ContextCacheBackend::Disabled => {}
            ContextCacheBackend::Memory => {
                if self.context_cache_ttl_seconds == 0 {
                    return Err(MemcoreError::ValidationError(
                        "MEMCORE_CONTEXT_CACHE_TTL_SECONDS must be greater than 0 when context cache backend is memory"
                            .to_string(),
                    ));
                }
                if self.context_cache_max_entries == 0 {
                    return Err(MemcoreError::ValidationError(
                        "MEMCORE_CONTEXT_CACHE_MAX_ENTRIES must be greater than 0 when context cache backend is memory"
                            .to_string(),
                    ));
                }
            }
            ContextCacheBackend::Redis => {
                if self.context_cache_ttl_seconds == 0 {
                    return Err(MemcoreError::ValidationError(
                        "MEMCORE_CONTEXT_CACHE_TTL_SECONDS must be greater than 0 when context cache backend is redis"
                            .to_string(),
                    ));
                }
                if self
                    .redis_url
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true)
                {
                    return Err(MemcoreError::ValidationError(
                        "MEMCORE_REDIS_URL is required when MEMCORE_CONTEXT_CACHE_BACKEND=redis"
                            .to_string(),
                    ));
                }
            }
        }

        if self.context_cache_key_prefix.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_CONTEXT_CACHE_KEY_PREFIX cannot be empty".to_string(),
            ));
        }

        if self.vector_backend == VectorBackend::Qdrant {
            if self.qdrant_url.trim().is_empty() {
                return Err(MemcoreError::ValidationError(
                    "MEMCORE_QDRANT_URL is required when MEMCORE_VECTOR_BACKEND=qdrant".to_string(),
                ));
            }
            if self.qdrant_collection.trim().is_empty() {
                return Err(MemcoreError::ValidationError(
                    "MEMCORE_QDRANT_COLLECTION cannot be empty when MEMCORE_VECTOR_BACKEND=qdrant"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }
}

/// Loads optional `.env` from the current directory or parents, then reads settings
/// from the process environment.
///
/// Missing `.env` is normal and does not fail startup. Variables already set in the
/// process environment (Docker, CI, systemd, etc.) are never overwritten by `.env`.
pub fn load_settings() -> MemcoreResult<Settings> {
    load_dotenv_if_present();
    Settings::from_env()
}

/// Loads `.env` when present. No-op when the file is missing.
fn load_dotenv_if_present() {
    let _ = dotenvy::dotenv();
}

fn read_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn read_env_optional(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        Ok(_) | Err(_) => None,
    }
}

fn parse_u16(key: &str, default: u16) -> MemcoreResult<u16> {
    match env::var(key) {
        Ok(value) => value.parse::<u16>().map_err(|_| {
            MemcoreError::ValidationError(format!("{key} must be a valid u16 port"))
        }),
        Err(_) => Ok(default),
    }
}

fn parse_u32(key: &str, default: u32) -> MemcoreResult<u32> {
    match env::var(key) {
        Ok(value) => value.parse::<u32>().map_err(|_| {
            MemcoreError::ValidationError(format!("{key} must be a valid unsigned integer"))
        }),
        Err(_) => Ok(default),
    }
}

fn parse_f32(key: &str, default: f32) -> MemcoreResult<f32> {
    match env::var(key) {
        Ok(value) => value.parse::<f32>().map_err(|_| {
            MemcoreError::ValidationError(format!("{key} must be a valid floating-point number"))
        }),
        Err(_) => Ok(default),
    }
}

fn parse_u64(key: &str, default: u64) -> MemcoreResult<u64> {
    match env::var(key) {
        Ok(value) => value.parse::<u64>().map_err(|_| {
            MemcoreError::ValidationError(format!("{key} must be a valid unsigned integer"))
        }),
        Err(_) => Ok(default),
    }
}

fn parse_usize(key: &str, default: usize) -> MemcoreResult<usize> {
    match env::var(key) {
        Ok(value) => value.parse::<usize>().map_err(|_| {
            MemcoreError::ValidationError(format!("{key} must be a valid unsigned integer"))
        }),
        Err(_) => Ok(default),
    }
}

fn parse_bool(key: &str, default: bool) -> MemcoreResult<bool> {
    match env::var(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(MemcoreError::ValidationError(format!(
                "{key} must be one of: true, false, 1, 0, yes, no"
            ))),
        },
        Err(_) => Ok(default),
    }
}

impl FromStr for Environment {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "development" | "dev" => Ok(Self::Development),
            "production" | "prod" => Ok(Self::Production),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_ENV value: {value}"
            ))),
        }
    }
}

impl FromStr for StorageMode {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "embedded" => Ok(Self::Embedded),
            "production" => Ok(Self::Production),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_STORAGE_MODE value: {value}"
            ))),
        }
    }
}

impl FromStr for VectorBackend {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "lancedb" => Ok(Self::LanceDb),
            "qdrant" => Ok(Self::Qdrant),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_VECTOR_BACKEND value: {value}"
            ))),
        }
    }
}

impl FromStr for FactBackend {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "sqlite" => Ok(Self::Sqlite),
            "postgres" => Ok(Self::Postgres),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_FACT_BACKEND value: {value}"
            ))),
        }
    }
}

impl EventBackend {
    pub fn default_for_fact_backend(fact_backend: &FactBackend) -> Self {
        match fact_backend {
            FactBackend::Mock => Self::Mock,
            FactBackend::Sqlite => Self::Sqlite,
            FactBackend::Postgres => Self::Postgres,
        }
    }
}

impl FromStr for EventBackend {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "sqlite" => Ok(Self::Sqlite),
            "postgres" => Ok(Self::Postgres),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_EVENT_BACKEND value: {value}"
            ))),
        }
    }
}

impl FromStr for LlmProviderKind {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "openai" => Ok(Self::OpenAi),
            "openrouter" => Ok(Self::OpenRouter),
            "anthropic" => Ok(Self::Anthropic),
            "groq" => Ok(Self::Groq),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_LLM_PROVIDER value: {value}"
            ))),
        }
    }
}

impl FromStr for EmbeddingProviderKind {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "openai" => Ok(Self::OpenAi),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_EMBEDDING_PROVIDER value: {value}"
            ))),
        }
    }
}

impl FromStr for ContextCacheBackend {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" => Ok(Self::Disabled),
            "memory" => Ok(Self::Memory),
            "redis" => Ok(Self::Redis),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_CONTEXT_CACHE_BACKEND value: {value}"
            ))),
        }
    }
}

impl FromStr for AuthMode {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dev" => Ok(Self::Dev),
            "database" => Ok(Self::Database),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_AUTH_MODE value: {value}"
            ))),
        }
    }
}

impl FromStr for LogFormat {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_LOG_FORMAT value: {value}"
            ))),
        }
    }
}

impl FromStr for LogLevel {
    type Err = MemcoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err(MemcoreError::ValidationError(format!(
                "Invalid MEMCORE_LOG_LEVEL value: {value}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    use super::{Environment, Settings, StorageMode, VectorBackend};

    const ENV_KEYS: [&str; 40] = [
        "MEMCORE_ENV",
        "MEMCORE_HOST",
        "MEMCORE_PORT",
        "MEMCORE_STORAGE_MODE",
        "MEMCORE_VECTOR_BACKEND",
        "MEMCORE_FACT_BACKEND",
        "MEMCORE_EVENT_BACKEND",
        "MEMCORE_DATABASE_URL",
        "MEMCORE_POSTGRES_URL",
        "MEMCORE_QDRANT_URL",
        "MEMCORE_QDRANT_COLLECTION",
        "MEMCORE_LANCEDB_PATH",
        "MEMCORE_LANCEDB_TABLE",
        "MEMCORE_LLM_PROVIDER",
        "MEMCORE_LLM_MODEL",
        "MEMCORE_EMBEDDING_PROVIDER",
        "MEMCORE_EMBEDDING_MODEL",
        "MEMCORE_ENABLE_PII_REDACTION",
        "MEMCORE_MIN_IMPORTANCE",
        "MEMCORE_AUTH_ENABLED",
        "MEMCORE_AUTH_MODE",
        "MEMCORE_DEV_API_KEY",
        "MEMCORE_API_KEY_PEPPER",
        "MEMCORE_RATE_LIMIT_ENABLED",
        "MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE",
        "MEMCORE_LOG_FORMAT",
        "MEMCORE_LOG_LEVEL",
        "MEMCORE_REQUEST_ID_HEADER",
        "MEMCORE_METRICS_ENABLED",
        "MEMCORE_RETENTION_ENABLED",
        "MEMCORE_FACT_RETENTION_DAYS",
        "MEMCORE_EVENT_RETENTION_DAYS",
        "MEMCORE_CONTEXT_CACHE_ENABLED",
        "MEMCORE_CONTEXT_CACHE_BACKEND",
        "MEMCORE_CONTEXT_CACHE_TTL_SECONDS",
        "MEMCORE_CONTEXT_CACHE_MAX_ENTRIES",
        "MEMCORE_CONTEXT_CACHE_KEY_PREFIX",
        "MEMCORE_REDIS_URL",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
    ];

    fn env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        previous: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvGuard {
        fn new() -> Self {
            let previous = ENV_KEYS
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect::<Vec<_>>();
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.previous {
                match value {
                    // SAFETY: tests are serialized with a process-wide mutex, and values
                    // are restored only within that lock's lifetime.
                    Some(v) => unsafe { std::env::set_var(key, v) },
                    // SAFETY: same justification as above; serialized mutation in tests.
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    fn clear_env() {
        for key in ENV_KEYS {
            // SAFETY: tests mutate env only while holding the process-wide mutex.
            unsafe { std::env::remove_var(key) };
        }
    }

    #[test]
    fn loads_defaults_when_env_is_missing() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        let settings = Settings::from_env().expect("defaults should load");
        assert_eq!(settings.environment, Environment::Development);
        assert_eq!(settings.port, 8080);
        assert_eq!(settings.storage_mode, StorageMode::Embedded);
        assert_eq!(settings.vector_backend, VectorBackend::LanceDb);
        assert_eq!(settings.min_importance, 0.55);
        assert!(settings.enable_pii_redaction);
        assert!(settings.auth_enabled);
        assert_eq!(settings.dev_api_key, "memcore_dev_key");
        assert_eq!(settings.fact_backend, super::FactBackend::Sqlite);
        assert_eq!(settings.event_backend, super::EventBackend::Sqlite);
        assert!(settings.rate_limit_enabled);
        assert_eq!(settings.rate_limit_requests_per_minute, 60);
        assert_eq!(settings.log_format, super::LogFormat::Json);
        assert_eq!(settings.log_level, super::LogLevel::Info);
        assert_eq!(settings.request_id_header, super::DEFAULT_REQUEST_ID_HEADER);
        assert!(settings.metrics_enabled);
        assert!(!settings.retention_enabled);
        assert_eq!(settings.fact_retention_days, 0);
        assert_eq!(settings.event_retention_days, 0);
    }

    #[test]
    fn context_cache_disabled_by_default() {
        let settings = Settings::default();
        assert!(!settings.context_cache_enabled);
        assert_eq!(settings.context_cache_backend, super::ContextCacheBackend::Disabled);
        assert_eq!(settings.context_cache_ttl_seconds, 300);
        assert_eq!(settings.context_cache_max_entries, 1000);
        assert_eq!(
            settings.context_cache_key_prefix,
            super::DEFAULT_CONTEXT_CACHE_KEY_PREFIX
        );
    }

    #[test]
    fn context_cache_memory_backend_parses_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_CONTEXT_CACHE_BACKEND", "memory");
        }

        let settings = Settings::from_env().expect("memory backend should load");
        assert!(settings.context_cache_enabled);
        assert_eq!(settings.context_cache_backend, super::ContextCacheBackend::Memory);
    }

    #[test]
    fn context_cache_disabled_backend_parses_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_CONTEXT_CACHE_BACKEND", "disabled");
        }

        let settings = Settings::from_env().expect("disabled backend should load");
        assert!(!settings.context_cache_enabled);
        assert_eq!(settings.context_cache_backend, super::ContextCacheBackend::Disabled);
    }

    #[test]
    fn context_cache_redis_backend_parses_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_CONTEXT_CACHE_BACKEND", "redis");
            std::env::set_var("MEMCORE_REDIS_URL", "redis://localhost:6379");
        }

        let settings = Settings::from_env().expect("redis backend should load");
        assert!(settings.context_cache_enabled);
        assert_eq!(settings.context_cache_backend, super::ContextCacheBackend::Redis);
        assert_eq!(settings.redis_url.as_deref(), Some("redis://localhost:6379"));
    }

    #[test]
    fn context_cache_redis_backend_requires_redis_url() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_CONTEXT_CACHE_BACKEND", "redis");
        }

        let error = Settings::from_env().expect_err("redis without url should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_REDIS_URL is required when MEMCORE_CONTEXT_CACHE_BACKEND=redis")
        );
    }

    #[test]
    fn fails_on_invalid_context_cache_backend() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_CONTEXT_CACHE_BACKEND", "not-a-backend");
        }

        let error = Settings::from_env().expect_err("invalid backend should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("Invalid MEMCORE_CONTEXT_CACHE_BACKEND value")
        );
    }

    #[test]
    fn legacy_context_cache_enabled_env_selects_memory_backend() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_CONTEXT_CACHE_ENABLED", "true");
        }

        let settings = Settings::from_env().expect("legacy enabled flag should load");
        assert!(settings.context_cache_enabled);
        assert_eq!(settings.context_cache_backend, super::ContextCacheBackend::Memory);
    }

    #[test]
    fn redis_password_not_exposed_in_sanitized_url_output() {
        use memcore_common::sanitize_redis_url_for_display;

        let sanitized =
            sanitize_redis_url_for_display("redis://:super_secret@localhost:6379/0");
        assert!(!sanitized.contains("super_secret"));
        assert!(sanitized.contains("***@"));
    }

    #[test]
    fn loads_context_cache_settings_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_CONTEXT_CACHE_BACKEND", "memory");
            std::env::set_var("MEMCORE_CONTEXT_CACHE_TTL_SECONDS", "120");
            std::env::set_var("MEMCORE_CONTEXT_CACHE_MAX_ENTRIES", "50");
            std::env::set_var("MEMCORE_CONTEXT_CACHE_KEY_PREFIX", "custom");
        }

        let settings = Settings::from_env().expect("context cache settings should load");
        assert!(settings.context_cache_enabled);
        assert_eq!(settings.context_cache_backend, super::ContextCacheBackend::Memory);
        assert_eq!(settings.context_cache_ttl_seconds, 120);
        assert_eq!(settings.context_cache_max_entries, 50);
        assert_eq!(settings.context_cache_key_prefix, "custom");
    }

    #[test]
    fn retention_disabled_by_default() {
        let settings = Settings::default();
        assert!(!settings.retention_enabled);
        assert_eq!(settings.fact_retention_days, 0);
        assert_eq!(settings.event_retention_days, 0);
    }

    #[test]
    fn loads_retention_settings_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_RETENTION_ENABLED", "true");
            std::env::set_var("MEMCORE_FACT_RETENTION_DAYS", "365");
            std::env::set_var("MEMCORE_EVENT_RETENTION_DAYS", "90");
        }

        let settings = Settings::from_env().expect("retention settings should load");
        assert!(settings.retention_enabled);
        assert_eq!(settings.fact_retention_days, 365);
        assert_eq!(settings.event_retention_days, 90);
    }

    #[test]
    fn fails_on_negative_retention_days() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_FACT_RETENTION_DAYS", "-1");
        }

        let error = Settings::from_env().expect_err("negative retention days should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_FACT_RETENTION_DAYS must be a valid unsigned integer")
        );
    }

    #[test]
    fn default_struct_uses_mock_backends() {
        let settings = Settings::default();
        assert_eq!(settings.fact_backend, super::FactBackend::Mock);
        assert_eq!(settings.event_backend, super::EventBackend::Mock);
        assert_eq!(settings.vector_backend, super::VectorBackend::Mock);
        assert_eq!(settings.auth_mode, super::AuthMode::Dev);
    }

    #[test]
    fn sqlite_memory_settings_use_in_memory_database() {
        let settings = Settings::sqlite_memory();
        assert_eq!(settings.fact_backend, super::FactBackend::Sqlite);
        assert_eq!(settings.event_backend, super::EventBackend::Sqlite);
        assert!(settings.database_url.contains(":memory:"));
    }

    #[test]
    fn event_backend_defaults_to_match_fact_backend() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_FACT_BACKEND", "postgres");
            std::env::set_var("MEMCORE_POSTGRES_URL", "postgres://localhost:5432/memcore");
        }

        let settings = Settings::from_env().expect("settings should load");
        assert_eq!(settings.fact_backend, super::FactBackend::Postgres);
        assert_eq!(settings.event_backend, super::EventBackend::Postgres);
    }

    #[test]
    fn database_auth_mode_requires_api_key_pepper() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_AUTH_MODE", "database");
        }

        let error = Settings::from_env().expect_err("database auth without pepper should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_API_KEY_PEPPER is required when MEMCORE_AUTH_MODE=database")
        );
    }

    #[test]
    fn fails_on_invalid_auth_mode() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_AUTH_MODE", "invalid-mode");
        }

        let error = Settings::from_env().expect_err("invalid auth mode should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("Invalid MEMCORE_AUTH_MODE value")
        );
    }

    #[test]
    fn loads_auth_settings_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_AUTH_ENABLED", "false");
            std::env::set_var("MEMCORE_DEV_API_KEY", "custom_dev_key");
        }

        let settings = Settings::from_env().expect("auth settings should load");
        assert!(!settings.auth_enabled);
        assert_eq!(settings.dev_api_key, "custom_dev_key");
    }

    #[test]
    fn loads_custom_values_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_ENV", "production");
            std::env::set_var("MEMCORE_PORT", "9090");
            std::env::set_var("MEMCORE_STORAGE_MODE", "production");
            std::env::set_var("MEMCORE_VECTOR_BACKEND", "qdrant");
            std::env::set_var("MEMCORE_FACT_BACKEND", "postgres");
            std::env::set_var("MEMCORE_POSTGRES_URL", "postgres://localhost:5432/memcore");
            std::env::set_var("MEMCORE_MIN_IMPORTANCE", "0.8");
            std::env::set_var("MEMCORE_ENABLE_PII_REDACTION", "false");
        }

        let settings = Settings::from_env().expect("custom settings should load");
        assert_eq!(settings.environment, Environment::Production);
        assert_eq!(settings.port, 9090);
        assert_eq!(settings.storage_mode, StorageMode::Production);
        assert_eq!(settings.vector_backend, VectorBackend::Qdrant);
        assert_eq!(settings.min_importance, 0.8);
        assert!(!settings.enable_pii_redaction);
    }

    #[test]
    fn fails_on_invalid_port() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_PORT", "not-a-port") };

        let error = Settings::from_env().expect_err("invalid port should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_PORT must be a valid u16 port")
        );
    }

    #[test]
    fn fails_on_invalid_min_importance() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_MIN_IMPORTANCE", "1.5") };

        let error = Settings::from_env().expect_err("out-of-range min importance should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_MIN_IMPORTANCE must be between 0.0 and 1.0")
        );
    }

    #[test]
    fn openai_provider_requires_api_key() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_LLM_PROVIDER", "openai");
        }

        let error = Settings::from_env().expect_err("openai without key should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(error.to_string().contains("OPENAI_API_KEY"));
    }

    #[test]
    fn openai_api_key_not_required_for_mock_providers() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        let settings = Settings::from_env().expect("mock providers should load without openai key");
        assert_eq!(settings.llm_provider, super::LlmProviderKind::Mock);
        assert!(settings.openai_api_key.is_none());
    }

    #[test]
    fn loads_rate_limit_settings_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_RATE_LIMIT_ENABLED", "false");
            std::env::set_var("MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE", "120");
        }

        let settings = Settings::from_env().expect("rate limit settings should load");
        assert!(!settings.rate_limit_enabled);
        assert_eq!(settings.rate_limit_requests_per_minute, 120);
    }

    #[test]
    fn fails_on_zero_rate_limit_requests_per_minute() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE", "0") };

        let error = Settings::from_env().expect_err("zero rate limit should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE must be greater than 0")
        );
    }

    #[test]
    fn fails_on_invalid_rate_limit_requests_per_minute() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE", "not-a-number") };

        let error = Settings::from_env().expect_err("invalid rate limit should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE must be a valid unsigned integer")
        );
    }

    #[test]
    fn postgres_event_backend_requires_postgres_url() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_FACT_BACKEND", "mock");
            std::env::set_var("MEMCORE_EVENT_BACKEND", "postgres");
        }

        let error = Settings::from_env().expect_err("postgres event without url should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_POSTGRES_URL is required when MEMCORE_EVENT_BACKEND=postgres")
        );
    }

    #[test]
    fn fails_on_invalid_event_backend() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_EVENT_BACKEND", "not-a-backend");
        }

        let error = Settings::from_env().expect_err("invalid event backend should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("Invalid MEMCORE_EVENT_BACKEND value")
        );
    }

    #[test]
    fn postgres_fact_backend_requires_postgres_url() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_FACT_BACKEND", "postgres");
        }

        let error = Settings::from_env().expect_err("postgres without url should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_POSTGRES_URL is required when MEMCORE_FACT_BACKEND=postgres")
        );
    }

    #[test]
    fn fails_on_invalid_log_format() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_LOG_FORMAT", "yaml") };

        let error = Settings::from_env().expect_err("invalid log format should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("Invalid MEMCORE_LOG_FORMAT value")
        );
    }

    #[test]
    fn fails_on_invalid_log_level() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_LOG_LEVEL", "verbose") };

        let error = Settings::from_env().expect_err("invalid log level should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("Invalid MEMCORE_LOG_LEVEL value")
        );
    }

    #[test]
    fn loads_observability_settings_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_LOG_FORMAT", "pretty");
            std::env::set_var("MEMCORE_LOG_LEVEL", "debug");
            std::env::set_var("MEMCORE_REQUEST_ID_HEADER", "X-Correlation-ID");
            std::env::set_var("MEMCORE_METRICS_ENABLED", "false");
        }

        let settings = Settings::from_env().expect("observability settings should load");
        assert_eq!(settings.log_format, super::LogFormat::Pretty);
        assert_eq!(settings.log_level, super::LogLevel::Debug);
        assert_eq!(settings.request_id_header, "X-Correlation-ID");
        assert!(!settings.metrics_enabled);
    }

    #[test]
    fn load_settings_succeeds_when_dotenv_file_is_missing() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        super::load_settings().expect("missing .env should not fail startup");
    }

    #[test]
    fn load_settings_reads_values_from_dotenv_file() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        let dir = std::env::temp_dir().join(format!("memcore-dotenv-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        std::fs::write(dir.join(".env"), "MEMCORE_PORT=9123\n").expect(".env should be written");

        let original = std::env::current_dir().expect("cwd should exist");
        std::env::set_current_dir(&dir).expect("chdir should succeed");
        let settings = super::load_settings().expect("settings should load from dotenv");
        std::env::set_current_dir(&original).expect("cwd should be restored");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(settings.port, 9123);
    }

    #[test]
    fn dotenv_does_not_override_existing_environment_variables() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        let dir = std::env::temp_dir().join(format!(
            "memcore-dotenv-override-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        std::fs::write(dir.join(".env"), "MEMCORE_PORT=9123\n").expect(".env should be written");

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_PORT", "7777") };

        let original = std::env::current_dir().expect("cwd should exist");
        std::env::set_current_dir(&dir).expect("chdir should succeed");
        let settings = super::load_settings().expect("settings should load");
        std::env::set_current_dir(&original).expect("cwd should be restored");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(settings.port, 7777);
    }

    #[test]
    fn qdrant_backend_requires_qdrant_url() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_VECTOR_BACKEND", "qdrant");
            std::env::set_var("MEMCORE_QDRANT_URL", "   ");
        }

        let error = Settings::from_env().expect_err("qdrant without url should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("MEMCORE_QDRANT_URL is required when MEMCORE_VECTOR_BACKEND=qdrant")
        );
    }

    #[test]
    fn qdrant_collection_defaults_to_memcore_vectors() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_VECTOR_BACKEND", "qdrant");
        }

        let settings = Settings::from_env().expect("qdrant settings should load");
        assert_eq!(settings.vector_backend, VectorBackend::Qdrant);
        assert_eq!(settings.qdrant_url, "http://localhost:6333");
        assert_eq!(settings.qdrant_collection, "memcore_vectors");
    }

    #[test]
    fn loads_qdrant_collection_from_env() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe {
            std::env::set_var("MEMCORE_VECTOR_BACKEND", "qdrant");
            std::env::set_var("MEMCORE_QDRANT_COLLECTION", "custom_vectors");
        }

        let settings = Settings::from_env().expect("qdrant collection should load");
        assert_eq!(settings.qdrant_collection, "custom_vectors");
    }

    #[test]
    fn fails_on_invalid_enum_value() {
        let _lock = env_test_lock()
            .lock()
            .expect("env test lock should not be poisoned");
        let _guard = EnvGuard::new();
        clear_env();

        // SAFETY: tests mutate env only while holding the process-wide mutex.
        unsafe { std::env::set_var("MEMCORE_VECTOR_BACKEND", "bad-backend") };

        let error = Settings::from_env().expect_err("invalid enum should fail");
        assert_eq!(error.code(), "validation_error");
        assert!(
            error
                .to_string()
                .contains("Invalid MEMCORE_VECTOR_BACKEND value")
        );
    }
}
