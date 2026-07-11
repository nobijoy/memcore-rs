//! Route label normalization (low cardinality).

use uuid::Uuid;

/// Prefer Axum matched path; otherwise normalize dynamic segments.
pub fn normalize_route(matched: Option<&str>, raw_path: &str) -> String {
    if let Some(matched) = matched.filter(|value| !value.is_empty()) {
        return matched.to_string();
    }
    normalize_raw_path(raw_path)
}

fn normalize_raw_path(path: &str) -> String {
    let trimmed = path.split('?').next().unwrap_or(path);
    let mut out = Vec::new();
    for (idx, segment) in trimmed.split('/').enumerate() {
        if idx == 0 && segment.is_empty() {
            continue;
        }
        if segment.is_empty() {
            continue;
        }
        out.push(normalize_segment(segment));
    }
    if out.is_empty() {
        return "/".to_string();
    }
    format!("/{}", out.join("/"))
}

fn normalize_segment(segment: &str) -> &str {
    if Uuid::parse_str(segment).is_ok() {
        return "{id}";
    }
    // Common tenant path segments that are high-cardinality when literal.
    if looks_like_user_id(segment) {
        return "{user_id}";
    }
    segment
}

fn looks_like_user_id(segment: &str) -> bool {
    // Heuristic only used when MatchedPath is unavailable.
    segment.starts_with("user_")
        || segment.starts_with("perf-user-")
        || segment.starts_with("smoke-test-")
        || (segment.len() > 24
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_matched_path() {
        assert_eq!(
            normalize_route(
                Some("/api/v1/users/{user_id}/memories"),
                "/api/v1/users/alice/memories"
            ),
            "/api/v1/users/{user_id}/memories"
        );
    }

    #[test]
    fn strips_query_string() {
        assert_eq!(normalize_route(None, "/health?x=1"), "/health");
    }

    #[test]
    fn normalizes_uuid_segments() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            normalize_route(None, &format!("/api/v1/api-keys/{id}")),
            "/api/v1/api-keys/{id}"
        );
    }
}
