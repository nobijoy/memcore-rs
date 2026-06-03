use axum::Json;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use memcore_common::MemcoreResult;
use memcore_core::TenantContext;

use crate::response::ErrorBody;

pub const ORG_HEADER: &str = "X-Organization-ID";

/// Organization scope extracted from `X-Organization-ID` (tenant middleware only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationContext {
    pub org_id: String,
}

impl OrganizationContext {
    pub fn tenant_with_user_id(&self, user_id: impl Into<String>) -> MemcoreResult<TenantContext> {
        TenantContext::new(self.org_id.clone(), user_id)
    }
}

/// Validates `X-Organization-ID` and attaches [`OrganizationContext`] to the request.
///
/// Runs after auth middleware on protected routes. Does not validate `user_id` (request body).
pub async fn require_organization(mut request: Request, next: Next) -> Response {
    match extract_organization(request.headers()) {
        Ok(org) => {
            request.extensions_mut().insert(org);
            next.run(request).await
        }
        Err(response) => response,
    }
}

fn extract_organization(headers: &HeaderMap) -> Result<OrganizationContext, Response> {
    let Some(value) = headers.get(ORG_HEADER) else {
        return Err(validation_response("missing X-Organization-ID header"));
    };

    let header_str = value
        .to_str()
        .map_err(|_| validation_response("X-Organization-ID must be valid UTF-8"))?;

    let org_id = header_str.trim();
    if org_id.is_empty() {
        return Err(validation_response("X-Organization-ID cannot be empty"));
    }

    Ok(OrganizationContext {
        org_id: org_id.to_string(),
    })
}

fn validation_response(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody::new("VALIDATION_ERROR", message)),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::{extract_organization, ORG_HEADER};

    #[test]
    fn missing_header_returns_error() {
        let headers = HeaderMap::new();
        assert!(extract_organization(&headers).is_err());
    }

    #[test]
    fn empty_header_returns_error() {
        let mut headers = HeaderMap::new();
        headers.insert(ORG_HEADER, "   ".parse().unwrap());
        assert!(extract_organization(&headers).is_err());
    }

    #[test]
    fn trims_whitespace_from_org_id() {
        let mut headers = HeaderMap::new();
        headers.insert(ORG_HEADER, "  org_abc  ".parse().unwrap());
        let org = extract_organization(&headers).expect("org should parse");
        assert_eq!(org.org_id, "org_abc");
    }
}
