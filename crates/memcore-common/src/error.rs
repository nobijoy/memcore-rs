use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MemcoreError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("rate limited")]
    RateLimited,
    #[error("provider error: {0}")]
    ProviderError(String),
    #[error("storage error: {0}")]
    StorageError(String),
    #[error("validation error: {0}")]
    ValidationError(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("quota exceeded: {message}")]
    QuotaExceeded {
        message: String,
        kind: String,
        limit: u64,
        current: u64,
        requested: u64,
    },
}

pub const PROVIDER_TIMEOUT_MESSAGE: &str = "provider operation timed out";
pub const PROVIDER_CIRCUIT_OPEN_MESSAGE: &str = "provider circuit is open";

pub type MemcoreResult<T> = Result<T, MemcoreError>;

impl MemcoreError {
    pub fn provider_timeout() -> Self {
        Self::Timeout(PROVIDER_TIMEOUT_MESSAGE.to_string())
    }

    pub fn is_provider_timeout(&self) -> bool {
        matches!(self, Self::Timeout(msg) if msg == PROVIDER_TIMEOUT_MESSAGE)
    }

    pub fn provider_circuit_open() -> Self {
        Self::ProviderError(PROVIDER_CIRCUIT_OPEN_MESSAGE.to_string())
    }

    pub fn is_provider_circuit_open(&self) -> bool {
        matches!(self, Self::ProviderError(msg) if msg == PROVIDER_CIRCUIT_OPEN_MESSAGE)
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::BadRequest(_) => "bad_request",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::RateLimited => "rate_limited",
            Self::ProviderError(msg) if msg == PROVIDER_CIRCUIT_OPEN_MESSAGE => {
                "provider_circuit_open"
            }
            Self::ProviderError(_) => "provider_error",
            Self::StorageError(_) => "storage_error",
            Self::ValidationError(_) => "validation_error",
            Self::Internal(_) => "internal",
            Self::Timeout(msg) if msg == PROVIDER_TIMEOUT_MESSAGE => "provider_timeout",
            Self::Timeout(_) => "timeout",
            Self::QuotaExceeded { .. } => "quota_exceeded",
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
    }

    pub fn quota_exceeded(
        message: impl Into<String>,
        kind: impl Into<String>,
        limit: u64,
        current: u64,
        requested: u64,
    ) -> Self {
        Self::QuotaExceeded {
            message: message.into(),
            kind: kind.into(),
            limit,
            current,
            requested,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemcoreError, MemcoreResult};

    #[test]
    fn display_message_includes_variant_text() {
        let error = MemcoreError::BadRequest("missing user_id".to_string());
        assert_eq!(error.to_string(), "bad request: missing user_id");
    }

    #[test]
    fn provider_timeout_uses_dedicated_error_code() {
        let error = MemcoreError::provider_timeout();
        assert_eq!(error.code(), "provider_timeout");
        assert!(error.is_provider_timeout());
    }

    #[test]
    fn provider_circuit_open_uses_dedicated_error_code() {
        let error = MemcoreError::provider_circuit_open();
        assert_eq!(error.code(), "provider_circuit_open");
        assert!(error.is_provider_circuit_open());
    }

    #[test]
    fn quota_exceeded_uses_dedicated_error_code() {
        let error = MemcoreError::quota_exceeded("limit exceeded", "MemoriesPerOrg", 10, 10, 1);
        assert_eq!(error.code(), "quota_exceeded");
    }

    #[test]
    fn code_mapping_matches_expected_values() {
        let error = MemcoreError::StorageError("db unavailable".to_string());
        assert_eq!(error.code(), "storage_error");
    }

    #[test]
    fn memcore_result_usage_works() {
        fn validate_user_id(user_id: &str) -> MemcoreResult<()> {
            if user_id.is_empty() {
                return Err(MemcoreError::ValidationError(
                    "user_id cannot be empty".to_string(),
                ));
            }
            Ok(())
        }

        assert!(validate_user_id("user_123").is_ok());

        let error = validate_user_id("").expect_err("empty user_id should return an error");
        assert_eq!(
            error,
            MemcoreError::ValidationError("user_id cannot be empty".to_string())
        );
    }
}
