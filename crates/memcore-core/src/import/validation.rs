use memcore_common::{MemcoreError, MemcoreResult};
use serde_json::Value;
use uuid::Uuid;

use crate::export::{UserMemoryExport, USER_EXPORT_FORMAT_VERSION};
use crate::{Fact, MemoryEvent, TenantContext};

const FORBIDDEN_METADATA_KEYS: &[&str] = &[
    "api_key",
    "key_hash",
    "raw_key",
    "bearer",
    "bearer_token",
    "secret",
];

/// Validates export envelope and tenant alignment before import.
pub fn validate_export_for_import(
    export: &UserMemoryExport,
    tenant: &TenantContext,
) -> MemcoreResult<()> {
    if export.format_version != USER_EXPORT_FORMAT_VERSION {
        return Err(MemcoreError::ValidationError(format!(
            "unsupported format_version: {}",
            export.format_version
        )));
    }

    if export.org_id != tenant.org_id {
        return Err(MemcoreError::ValidationError(
            "export org_id does not match tenant org_id".to_string(),
        ));
    }

    if export.user_id != tenant.user_id {
        return Err(MemcoreError::ValidationError(
            "export user_id does not match path user_id".to_string(),
        ));
    }

    Ok(())
}

/// Validates a single exported fact for import.
pub fn validate_fact_for_import(fact: &Fact, tenant: &TenantContext) -> MemcoreResult<()> {
    if fact.org_id != tenant.org_id || fact.user_id != tenant.user_id {
        return Err(MemcoreError::ValidationError(
            "fact org_id/user_id does not match import tenant".to_string(),
        ));
    }

    if fact.content.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "fact content cannot be empty".to_string(),
        ));
    }

    if !(0.0..=1.0).contains(&fact.confidence) {
        return Err(MemcoreError::ValidationError(
            "fact confidence must be between 0.0 and 1.0".to_string(),
        ));
    }

    if !(0.0..=1.0).contains(&fact.importance) {
        return Err(MemcoreError::ValidationError(
            "fact importance must be between 0.0 and 1.0".to_string(),
        ));
    }

    if contains_forbidden_secret_fields(&fact.metadata) {
        return Err(MemcoreError::ValidationError(
            "fact metadata contains forbidden secret fields".to_string(),
        ));
    }

    Ok(())
}

/// Validates a single exported memory event for import.
pub fn validate_event_for_import(event: &MemoryEvent, tenant: &TenantContext) -> MemcoreResult<()> {
    if event.org_id != tenant.org_id || event.user_id != tenant.user_id {
        return Err(MemcoreError::ValidationError(
            "memory event org_id/user_id does not match import tenant".to_string(),
        ));
    }

    if contains_forbidden_secret_fields(&event.metadata) {
        return Err(MemcoreError::ValidationError(
            "memory event metadata contains forbidden secret fields".to_string(),
        ));
    }

    Ok(())
}

/// Returns true when JSON contains keys that must never be imported (API keys, hashes, tokens).
pub fn contains_forbidden_secret_fields(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                let normalized = key.to_ascii_lowercase();
                if FORBIDDEN_METADATA_KEYS
                    .iter()
                    .any(|forbidden| normalized.contains(forbidden))
                {
                    return true;
                }
                if contains_forbidden_secret_fields(nested) {
                    return true;
                }
            }
            false
        }
        Value::Array(items) => items.iter().any(contains_forbidden_secret_fields),
        _ => false,
    }
}

/// Resolves the fact id to use on import. In append mode, generates a new id when the
/// exported id already exists for this tenant.
pub fn resolve_import_fact_id(
    exported_id: Uuid,
    id_exists: bool,
    mode: super::ImportMode,
) -> Uuid {
    if id_exists && matches!(mode, super::ImportMode::Append) {
        Uuid::new_v4()
    } else {
        exported_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportMode;
    use crate::{MemorySource, MemoryType};
    use chrono::Utc;
    use serde_json::json;

    fn tenant() -> TenantContext {
        TenantContext::new("org_a", "user_a").expect("tenant")
    }

    fn sample_fact() -> Fact {
        Fact::new(
            Uuid::new_v4(),
            "org_a",
            "user_a",
            MemoryType::Profile,
            "hello",
            None,
            MemorySource::UserMessage,
            0.9,
            0.8,
            None,
            None,
            Utc::now(),
            Utc::now(),
            json!({}),
        )
        .expect("fact")
    }

    #[test]
    fn forbidden_metadata_keys_are_detected() {
        assert!(contains_forbidden_secret_fields(&json!({ "api_key": "x" })));
        assert!(contains_forbidden_secret_fields(&json!({ "nested": { "key_hash": "y" } })));
        assert!(!contains_forbidden_secret_fields(&json!({ "source": "import" })));
    }

    #[test]
    fn resolve_import_fact_id_regenerates_on_append_collision() {
        let original = Uuid::new_v4();
        let resolved = resolve_import_fact_id(original, true, ImportMode::Append);
        assert_ne!(resolved, original);
    }

    #[test]
    fn validate_fact_rejects_mismatched_tenant() {
        let mut fact = sample_fact();
        fact.user_id = "other".to_string();
        let err = validate_fact_for_import(&fact, &tenant()).expect_err("should fail");
        assert!(matches!(err, MemcoreError::ValidationError(_)));
    }
}
