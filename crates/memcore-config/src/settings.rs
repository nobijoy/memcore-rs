use std::env;
use std::str::FromStr;

use memcore_common::{MemcoreError, MemcoreResult};

const MEMCORE_ENV: &str = "MEMCORE_ENV";
const MEMCORE_HOST: &str = "MEMCORE_HOST";
const MEMCORE_PORT: &str = "MEMCORE_PORT";
const MEMCORE_STORAGE_MODE: &str = "MEMCORE_STORAGE_MODE";
const MEMCORE_VECTOR_BACKEND: &str = "MEMCORE_VECTOR_BACKEND";
const MEMCORE_FACT_BACKEND: &str = "MEMCORE_FACT_BACKEND";
const MEMCORE_DATABASE_URL: &str = "MEMCORE_DATABASE_URL";
const MEMCORE_POSTGRES_URL: &str = "MEMCORE_POSTGRES_URL";
const MEMCORE_QDRANT_URL: &str = "MEMCORE_QDRANT_URL";
const MEMCORE_LANCEDB_PATH: &str = "MEMCORE_LANCEDB_PATH";
const MEMCORE_LANCEDB_TABLE: &str = "MEMCORE_LANCEDB_TABLE";
const MEMCORE_LLM_PROVIDER: &str = "MEMCORE_LLM_PROVIDER";
const MEMCORE_LLM_MODEL: &str = "MEMCORE_LLM_MODEL";
const MEMCORE_EMBEDDING_PROVIDER: &str = "MEMCORE_EMBEDDING_PROVIDER";
const MEMCORE_EMBEDDING_MODEL: &str = "MEMCORE_EMBEDDING_MODEL";
const MEMCORE_ENABLE_PII_REDACTION: &str = "MEMCORE_ENABLE_PII_REDACTION";
const MEMCORE_MIN_IMPORTANCE: &str = "MEMCORE_MIN_IMPORTANCE";
const MEMCORE_AUTH_ENABLED: &str = "MEMCORE_AUTH_ENABLED";
const MEMCORE_DEV_API_KEY: &str = "MEMCORE_DEV_API_KEY";

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
    pub database_url: String,
    pub postgres_url: Option<String>,
    pub qdrant_url: String,
    pub lancedb_path: String,
    pub lancedb_table: String,
    pub llm_provider: LlmProviderKind,
    pub llm_model: String,
    pub embedding_provider: EmbeddingProviderKind,
    pub embedding_model: String,
    pub enable_pii_redaction: bool,
    pub min_importance: f32,
    /// Temporary development auth toggle. Production will use hashed keys from storage.
    pub auth_enabled: bool,
    /// Temporary plaintext dev API key. Do not log this value.
    pub dev_api_key: String,
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
            database_url: "sqlite://./data/memcore.db".to_string(),
            postgres_url: None,
            qdrant_url: "http://localhost:6333".to_string(),
            lancedb_path: "./data/lancedb".to_string(),
            lancedb_table: "memcore_vectors".to_string(),
            llm_provider: LlmProviderKind::Mock,
            llm_model: "mock-llm".to_string(),
            embedding_provider: EmbeddingProviderKind::Mock,
            embedding_model: "mock-embedding".to_string(),
            enable_pii_redaction: true,
            min_importance: 0.55,
            auth_enabled: true,
            dev_api_key: "memcore_dev_key".to_string(),
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
        let database_url = read_env_or(MEMCORE_DATABASE_URL, &defaults.database_url);
        let postgres_url = read_env_optional(MEMCORE_POSTGRES_URL);
        let qdrant_url = read_env_or(MEMCORE_QDRANT_URL, &defaults.qdrant_url);
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
        let dev_api_key = read_env_or(MEMCORE_DEV_API_KEY, &defaults.dev_api_key);

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
            database_url,
            postgres_url,
            qdrant_url,
            lancedb_path,
            lancedb_table,
            llm_provider,
            llm_model,
            embedding_provider,
            embedding_model,
            enable_pii_redaction,
            min_importance,
            auth_enabled,
            dev_api_key,
        };

        settings.validate()?;
        Ok(settings)
    }

    /// In-memory SQLite settings for integration tests.
    pub fn sqlite_memory() -> Self {
        Self {
            fact_backend: FactBackend::Sqlite,
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

        if self.qdrant_url.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_QDRANT_URL cannot be empty".to_string(),
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

        if self.auth_enabled && self.dev_api_key.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "MEMCORE_DEV_API_KEY cannot be empty when MEMCORE_AUTH_ENABLED=true".to_string(),
            ));
        }

        if self.storage_mode == StorageMode::Production {
            if self.fact_backend == FactBackend::Postgres
                && self
                    .postgres_url
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true)
            {
                return Err(MemcoreError::ValidationError(
                    "MEMCORE_POSTGRES_URL is required when production mode uses postgres"
                        .to_string(),
                ));
            }

            if self.vector_backend == VectorBackend::Qdrant && self.qdrant_url.trim().is_empty() {
                return Err(MemcoreError::ValidationError(
                    "MEMCORE_QDRANT_URL is required when production mode uses qdrant".to_string(),
                ));
            }
        }

        Ok(())
    }
}

pub fn load_settings() -> MemcoreResult<Settings> {
    Settings::from_env()
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

fn parse_f32(key: &str, default: f32) -> MemcoreResult<f32> {
    match env::var(key) {
        Ok(value) => value.parse::<f32>().map_err(|_| {
            MemcoreError::ValidationError(format!("{key} must be a valid floating-point number"))
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    use super::{Environment, Settings, StorageMode, VectorBackend};

    const ENV_KEYS: [&str; 19] = [
        "MEMCORE_ENV",
        "MEMCORE_HOST",
        "MEMCORE_PORT",
        "MEMCORE_STORAGE_MODE",
        "MEMCORE_VECTOR_BACKEND",
        "MEMCORE_FACT_BACKEND",
        "MEMCORE_DATABASE_URL",
        "MEMCORE_POSTGRES_URL",
        "MEMCORE_QDRANT_URL",
        "MEMCORE_LANCEDB_PATH",
        "MEMCORE_LANCEDB_TABLE",
        "MEMCORE_LLM_PROVIDER",
        "MEMCORE_LLM_MODEL",
        "MEMCORE_EMBEDDING_PROVIDER",
        "MEMCORE_EMBEDDING_MODEL",
        "MEMCORE_ENABLE_PII_REDACTION",
        "MEMCORE_MIN_IMPORTANCE",
        "MEMCORE_AUTH_ENABLED",
        "MEMCORE_DEV_API_KEY",
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
    }

    #[test]
    fn default_struct_uses_mock_backends() {
        let settings = Settings::default();
        assert_eq!(settings.fact_backend, super::FactBackend::Mock);
        assert_eq!(settings.vector_backend, super::VectorBackend::Mock);
    }

    #[test]
    fn sqlite_memory_settings_use_in_memory_database() {
        let settings = Settings::sqlite_memory();
        assert_eq!(settings.fact_backend, super::FactBackend::Sqlite);
        assert!(settings.database_url.contains(":memory:"));
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
