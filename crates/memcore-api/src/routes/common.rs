use axum::Json;
use axum::response::{IntoResponse, Response};
use memcore_common::MemcoreError;

use crate::response::ErrorBody;

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
