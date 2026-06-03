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
}

pub type MemcoreResult<T> = Result<T, MemcoreError>;

impl MemcoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::BadRequest(_) => "bad_request",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::RateLimited => "rate_limited",
            Self::ProviderError(_) => "provider_error",
            Self::StorageError(_) => "storage_error",
            Self::ValidationError(_) => "validation_error",
            Self::Internal(_) => "internal",
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
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
