use axum::Json;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use memcore_common::MemcoreError;

use crate::response::ErrorBody;

pub const ORG_HEADER: &str = "X-Organization-ID";

pub fn org_id_from_headers(headers: &HeaderMap) -> Result<String, MemcoreError> {
    let value = headers
        .get(ORG_HEADER)
        .ok_or_else(|| {
            MemcoreError::ValidationError(format!("{ORG_HEADER} header is required"))
        })?
        .to_str()
        .map_err(|_| {
            MemcoreError::ValidationError(format!("{ORG_HEADER} header must be valid UTF-8"))
        })?;

    let org_id = value.trim();
    if org_id.is_empty() {
        return Err(MemcoreError::ValidationError(format!(
            "{ORG_HEADER} header is required"
        )));
    }

    Ok(org_id.to_string())
}

#[derive(Debug)]
pub struct ApiError((axum::http::StatusCode, Json<ErrorBody>));

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = self.0;
        (status, body).into_response()
    }
}

impl From<MemcoreError> for ApiError {
    fn from(error: MemcoreError) -> Self {
        let (status, body) = ErrorBody::from_memcore_error(error);
        Self((status, Json(body)))
    }
}
