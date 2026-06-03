use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::response::ErrorBody;
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
        Err(response) => response,
    }
}

fn validate_authorization(headers: &HeaderMap, expected_key: &str) -> Result<(), Response> {
    let header_value = headers
        .get(AUTHORIZATION)
        .ok_or_else(|| unauthorized_response("missing authorization header"))?;

    let header_str = header_value
        .to_str()
        .map_err(|_| unauthorized_response("invalid authorization header"))?;

    let token = header_str
        .strip_prefix(BEARER_PREFIX)
        .ok_or_else(|| unauthorized_response("invalid authorization header"))?;

    if token.is_empty() {
        return Err(unauthorized_response("invalid authorization header"));
    }

    if token != expected_key {
        return Err(unauthorized_response("invalid api key"));
    }

    Ok(())
}

fn unauthorized_response(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody::new("UNAUTHORIZED", message)),
    )
        .into_response()
}
