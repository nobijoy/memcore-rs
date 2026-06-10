use axum::http::StatusCode;
use memcore_common::MemcoreError;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.into(),
                message: message.into(),
                request_id: None,
            },
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.error.request_id = request_id;
        self
    }

    pub fn from_memcore_error(error: MemcoreError) -> (StatusCode, Self) {
        let status = match &error {
            MemcoreError::Unauthorized => StatusCode::UNAUTHORIZED,
            MemcoreError::Forbidden => StatusCode::FORBIDDEN,
            MemcoreError::BadRequest(_) => StatusCode::BAD_REQUEST,
            MemcoreError::NotFound(_) => StatusCode::NOT_FOUND,
            MemcoreError::Conflict(_) => StatusCode::CONFLICT,
            MemcoreError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            MemcoreError::ProviderError(_) => StatusCode::BAD_GATEWAY,
            MemcoreError::StorageError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            MemcoreError::ValidationError(_) => StatusCode::BAD_REQUEST,
            MemcoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let code = match &error {
            MemcoreError::Unauthorized => "UNAUTHORIZED",
            MemcoreError::Forbidden => "FORBIDDEN",
            MemcoreError::BadRequest(_) => "BAD_REQUEST",
            MemcoreError::NotFound(_) => "NOT_FOUND",
            MemcoreError::Conflict(_) => "CONFLICT",
            MemcoreError::RateLimited => "RATE_LIMITED",
            MemcoreError::ProviderError(_) => "PROVIDER_ERROR",
            MemcoreError::StorageError(_) => "STORAGE_ERROR",
            MemcoreError::ValidationError(_) => "VALIDATION_ERROR",
            MemcoreError::Internal(_) => "INTERNAL_ERROR",
        };

        (status, Self::new(code, api_error_message(&error)))
    }
}

fn api_error_message(error: &MemcoreError) -> String {
    match error {
        MemcoreError::ValidationError(message)
        | MemcoreError::BadRequest(message)
        | MemcoreError::NotFound(message)
        | MemcoreError::Conflict(message)
        | MemcoreError::ProviderError(message)
        | MemcoreError::StorageError(message)
        | MemcoreError::Internal(message) => message.clone(),
        MemcoreError::Unauthorized => "unauthorized".to_string(),
        MemcoreError::Forbidden => "forbidden".to_string(),
        MemcoreError::RateLimited => "rate limit exceeded".to_string(),
    }
}
