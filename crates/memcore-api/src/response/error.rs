use axum::http::StatusCode;
use memcore_common::{MemcoreError, safe_error_message};
use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.into(),
                message: message.into(),
                details: None,
                request_id: None,
            },
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.error.details = Some(details);
        self
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
            MemcoreError::MigrationError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            MemcoreError::ValidationError(_) => StatusCode::BAD_REQUEST,
            MemcoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            MemcoreError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            MemcoreError::QuotaExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
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
            MemcoreError::MigrationError(_) => "MIGRATION_ERROR",
            MemcoreError::ValidationError(_) => "VALIDATION_ERROR",
            MemcoreError::Internal(_) => "INTERNAL_ERROR",
            MemcoreError::Timeout(_) => "TIMEOUT",
            MemcoreError::QuotaExceeded { .. } => "QUOTA_EXCEEDED",
        };

        let body = Self::new(code, safe_error_message(&error));
        let body = match &error {
            MemcoreError::QuotaExceeded {
                kind,
                limit,
                current,
                requested,
                ..
            } => body.with_details(json!({
                "kind": kind,
                "limit": limit,
                "current": current,
                "requested": requested,
            })),
            _ => body,
        };

        (status, body)
    }
}
