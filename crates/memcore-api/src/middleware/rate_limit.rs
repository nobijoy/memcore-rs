use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Extension, Request, State};
use axum::http::{HeaderValue, StatusCode, header::RETRY_AFTER};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::middleware::OrganizationContext;
use crate::response::ErrorBody;
use crate::state::AppState;

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    Allowed,
    Limited { retry_after_secs: u64 },
}

#[derive(Debug)]
struct RateLimitWindow {
    window_start: Instant,
    count: u32,
}

/// In-memory fixed-window rate limiter keyed by organization id.
#[derive(Debug)]
pub struct RateLimiter {
    enabled: bool,
    requests_per_minute: u32,
    windows: Mutex<HashMap<String, RateLimitWindow>>,
}

impl RateLimiter {
    pub fn new(enabled: bool, requests_per_minute: u32) -> Self {
        Self {
            enabled,
            requests_per_minute,
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn check(&self, org_id: &str) -> RateLimitDecision {
        if !self.enabled {
            return RateLimitDecision::Allowed;
        }

        let now = Instant::now();
        let mut windows = self
            .windows
            .lock()
            .expect("rate limiter mutex should not be poisoned");

        if windows.len() > 1_024 {
            windows.retain(|_, window| now.duration_since(window.window_start) < WINDOW * 2);
        }

        let entry = windows
            .entry(org_id.to_string())
            .or_insert(RateLimitWindow {
                window_start: now,
                count: 0,
            });

        if now.duration_since(entry.window_start) >= WINDOW {
            entry.window_start = now;
            entry.count = 0;
        }

        if entry.count >= self.requests_per_minute {
            let elapsed = now.duration_since(entry.window_start);
            let retry_after_secs = WINDOW.saturating_sub(elapsed).as_secs().max(1);
            return RateLimitDecision::Limited { retry_after_secs };
        }

        entry.count += 1;
        RateLimitDecision::Allowed
    }
}

/// Enforces per-organization request limits on protected routes.
///
/// Runs after auth and tenant middleware; keys limits by `OrganizationContext.org_id`.
pub async fn enforce_rate_limit(
    State(state): State<AppState>,
    Extension(organization): Extension<OrganizationContext>,
    request: Request,
    next: Next,
) -> Response {
    match state.rate_limiter.check(&organization.org_id) {
        RateLimitDecision::Allowed => next.run(request).await,
        RateLimitDecision::Limited { retry_after_secs } => rate_limited_response(retry_after_secs),
    }
}

fn rate_limited_response(retry_after_secs: u64) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(ErrorBody::new("RATE_LIMITED", "rate limit exceeded")),
    )
        .into_response();

    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        response.headers_mut().insert(RETRY_AFTER, value);
    }

    response
}

#[cfg(test)]
mod tests {
    use super::{RateLimitDecision, RateLimiter};

    #[test]
    fn disabled_limiter_allows_all_requests() {
        let limiter = RateLimiter::new(false, 2);
        for _ in 0..5 {
            assert_eq!(limiter.check("org_a"), RateLimitDecision::Allowed);
        }
    }

    #[test]
    fn limiter_blocks_after_threshold() {
        let limiter = RateLimiter::new(true, 2);
        assert_eq!(limiter.check("org_a"), RateLimitDecision::Allowed);
        assert_eq!(limiter.check("org_a"), RateLimitDecision::Allowed);
        assert!(matches!(
            limiter.check("org_a"),
            RateLimitDecision::Limited { .. }
        ));
    }

    #[test]
    fn limiter_is_scoped_by_org_id() {
        let limiter = RateLimiter::new(true, 1);
        assert_eq!(limiter.check("org_a"), RateLimitDecision::Allowed);
        assert!(matches!(
            limiter.check("org_a"),
            RateLimitDecision::Limited { .. }
        ));
        assert_eq!(limiter.check("org_b"), RateLimitDecision::Allowed);
    }
}
