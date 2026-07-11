/// Best-effort redaction of secrets and connection strings in error messages.
///
/// Intended for infrastructure errors. Do not over-sanitize ordinary validation messages.
pub fn sanitize_error_message(message: &str) -> String {
    let mut sanitized = message.to_string();

    for scheme in [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "redis://",
        "rediss://",
        "sqlite://",
    ] {
        sanitized = redact_url_scheme(&sanitized, scheme);
    }

    // Bare `sqlite:` paths (without `//`) after handling `sqlite://`.
    sanitized = redact_url_scheme(&sanitized, "sqlite:");

    sanitized = redact_prefixed_token(&sanitized, "Bearer ");
    sanitized = redact_prefixed_token(&sanitized, "bearer ");
    sanitized = redact_prefixed_token(&sanitized, "sk-");
    sanitized = redact_prefixed_token(&sanitized, "mc_live_");
    sanitized = redact_key_value(&sanitized, "api_key");
    sanitized = redact_key_value(&sanitized, "password");
    sanitized = redact_key_value(&sanitized, "token");

    sanitized
}

fn redact_url_scheme(message: &str, scheme: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let scheme_lower = scheme.to_ascii_lowercase();
    let mut out = String::with_capacity(message.len());
    let mut idx = 0;

    while let Some(rel) = lower[idx..].find(&scheme_lower) {
        let start = idx + rel;
        out.push_str(&message[idx..start]);
        out.push_str(scheme);
        out.push_str("***");
        let after = start + scheme.len();
        let end = message[after..]
            .find(char::is_whitespace)
            .map(|offset| after + offset)
            .unwrap_or(message.len());
        idx = end;
    }
    out.push_str(&message[idx..]);
    out
}

fn redact_prefixed_token(message: &str, prefix: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut out = String::with_capacity(message.len());
    let mut idx = 0;

    while let Some(rel) = lower[idx..].find(&prefix_lower) {
        let start = idx + rel;
        out.push_str(&message[idx..start]);
        out.push_str(&message[start..start + prefix.len()]);
        out.push_str("***");
        let value_start = start + prefix.len();
        let value_end = message[value_start..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | '}'))
            .map(|offset| value_start + offset)
            .unwrap_or(message.len());
        idx = value_end;
    }
    out.push_str(&message[idx..]);
    out
}

fn redact_key_value(message: &str, key: &str) -> String {
    let patterns = [format!("{key}="), format!("{key}:")];
    let mut current = message.to_string();
    for pattern in patterns {
        current = redact_after_pattern(&current, &pattern);
    }
    current
}

fn redact_after_pattern(message: &str, pattern: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let pattern_lower = pattern.to_ascii_lowercase();
    let mut out = String::with_capacity(message.len());
    let mut idx = 0;

    while let Some(rel) = lower[idx..].find(&pattern_lower) {
        let start = idx + rel;
        let value_start = start + pattern.len();
        out.push_str(&message[idx..value_start]);
        let trimmed = message[value_start..]
            .chars()
            .take_while(|ch| ch.is_whitespace() || matches!(*ch, '"' | '\''))
            .count();
        let token_start = value_start + trimmed;
        out.push_str(&message[value_start..token_start]);
        out.push_str("***");
        let token_end = message[token_start..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | '}'))
            .map(|offset| token_start + offset)
            .unwrap_or(message.len());
        idx = token_end;
    }
    out.push_str(&message[idx..]);
    out
}

#[cfg(test)]
mod tests {
    use super::sanitize_error_message;

    #[test]
    fn redacts_database_and_redis_urls() {
        let message = sanitize_error_message(
            "connect failed postgres://user:secret@db:5432/memcore redis://:pass@localhost:6379/0",
        );
        assert!(!message.contains("secret"));
        assert!(!message.contains(":pass@"));
        assert!(message.contains("postgres://***"));
        assert!(message.contains("redis://***"));
    }

    #[test]
    fn redacts_bearer_and_api_keys() {
        let message =
            sanitize_error_message("auth failed Bearer sk-live-abcdef api_key=supersecret");
        assert!(!message.contains("sk-live-abcdef"));
        assert!(!message.contains("supersecret"));
        assert!(message.contains("Bearer ***"));
    }

    #[test]
    fn leaves_ordinary_validation_messages() {
        let message = sanitize_error_message("user_id cannot be empty");
        assert_eq!(message, "user_id cannot be empty");
    }
}
