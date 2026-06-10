use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use uuid::Uuid;

/// Correlation ID attached to each HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

impl RequestId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn resolve_request_id(headers: &HeaderMap, header_name: &str) -> RequestId {
    headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| RequestId::new(value))
        .unwrap_or_else(RequestId::generate)
}

pub fn request_id_from_extensions(request: &Request) -> Option<RequestId> {
    request.extensions().get::<RequestId>().cloned()
}

pub fn insert_response_request_id_header(
    response: &mut axum::response::Response,
    header_name: &str,
    request_id: &RequestId,
) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::try_from(header_name),
        HeaderValue::from_str(request_id.as_str()),
    ) {
        response.headers_mut().insert(name, value);
    }
}
