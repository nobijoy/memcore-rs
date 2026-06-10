use axum::http::StatusCode;

use memcore_core::ApiKeyScope;

use super::AuthContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: &'static str,
}

pub fn ensure_scope(auth: &AuthContext, scope: ApiKeyScope) -> Result<(), ScopeError> {
    if auth.has_scope(scope) {
        Ok(())
    } else {
        Err(ScopeError {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN",
            message: "insufficient api key scope",
        })
    }
}
