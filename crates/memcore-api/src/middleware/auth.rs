use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::middleware::Next;
use axum::response::Response;

use crate::observability::{attach_error_code, error_response};
use crate::state::AppState;

const BEARER_PREFIX: &str = "Bearer ";

pub async fn require_api_key(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !state.settings.auth_enabled {
        return next.run(request).await;
    }

    match validate_authorization(request.headers(), &state.settings.dev_api_key) {
        Ok(()) => next.run(request).await,
        Err((status, code, message)) => {
            let mut response = error_response(status, code, message, &request);
            attach_error_code(&mut response, code);
            response
        }
    }
}

fn validate_authorization(
    headers: &HeaderMap,
    expected_key: &str,
) -> Result<(), (StatusCode, &'static str, &'static str)> {
    let header_value = headers
        .get(AUTHORIZATION)
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "missing authorization header",
            )
        })?;

    let header_str = header_value.to_str().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "invalid authorization header",
        )
    })?;

    let token = header_str.strip_prefix(BEARER_PREFIX).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "invalid authorization header",
        )
    })?;

    if token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "invalid authorization header",
        ));
    }

    if token != expected_key {
        return Err((
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "invalid api key",
        ));
    }

    Ok(())
}
