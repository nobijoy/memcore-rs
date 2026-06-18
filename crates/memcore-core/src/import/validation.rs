use memcore_common::{MemcoreError, MemcoreResult};
use serde_json::Value;
use uuid::Uuid;

use crate::export::{USER_EXPORT_FORMAT_VERSION, UserMemoryExport};
use crate::import::{ImportValidationIssue, ImportValidationSummary};
use crate::{Fact, MemoryEvent, TenantContext};

const FORBIDDEN_METADATA_KEYS: &[&str] = &[
    "api_key",
    "key_hash",
    "raw_key",
    "bearer",
    "bearer_token",
    "secret",
];

fn validation_issue(
    code: &str,
    message: impl Into<String>,
    path: Option<String>,
) -> ImportValidationIssue {
    ImportValidationIssue {
        code: code.to_string(),
        message: message.into(),
        path,
    }
}

/// Collects all import validation issues without performing writes.
pub fn collect_import_validation(
    export: &UserMemoryExport,
    tenant: &TenantContext,
    restore_events: bool,
) -> ImportValidationSummary {
    let mut errors = Vec::new();
    let warnings = Vec::new();

    if export.format_version != USER_EXPORT_FORMAT_VERSION {
        errors.push(validation_issue(
            "UNSUPPORTED_FORMAT_VERSION",
            format!("unsupported format_version: {}", export.format_version),
            Some("export.format_version".to_string()),
        ));
    }

    if export.org_id != tenant.org_id {
        errors.push(validation_issue(
            "ORG_ID_MISMATCH",
            "export org_id does not match tenant org_id",
            Some("export.org_id".to_string()),
        ));
    }

    if export.user_id != tenant.user_id {
        errors.push(validation_issue(
            "USER_ID_MISMATCH",
            "export user_id does not match path user_id",
            Some("export.user_id".to_string()),
        ));
    }

    for (index, fact) in export.facts.iter().enumerate() {
        collect_fact_validation_issues(fact, tenant, index, &mut errors);
    }

    if restore_events {
        for (index, event) in export.memory_events.iter().enumerate() {
            collect_event_validation_issues(event, tenant, index, &mut errors);
        }
    }

    ImportValidationSummary {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

fn collect_fact_validation_issues(
    fact: &Fact,
    tenant: &TenantContext,
    index: usize,
    errors: &mut Vec<ImportValidationIssue>,
) {
    let base_path = format!("export.facts[{index}]");

    if fact.org_id != tenant.org_id || fact.user_id != tenant.user_id {
        errors.push(validation_issue(
            "FACT_TENANT_MISMATCH",
            "fact org_id/user_id does not match import tenant",
            Some(base_path.clone()),
        ));
    }

    if fact.content.trim().is_empty() {
        errors.push(validation_issue(
            "EMPTY_FACT_CONTENT",
            "fact content cannot be empty",
            Some(format!("{base_path}.content")),
        ));
    }

    if !(0.0..=1.0).contains(&fact.confidence) {
        errors.push(validation_issue(
            "INVALID_CONFIDENCE",
            "fact confidence must be between 0.0 and 1.0",
            Some(format!("{base_path}.confidence")),
        ));
    }

    if !(0.0..=1.0).contains(&fact.importance) {
        errors.push(validation_issue(
            "INVALID_IMPORTANCE",
            "fact importance must be between 0.0 and 1.0",
            Some(format!("{base_path}.importance")),
        ));
    }

    if contains_forbidden_secret_fields(&fact.metadata) {
        errors.push(validation_issue(
            "FORBIDDEN_SECRET_METADATA",
            "fact metadata contains forbidden secret fields",
            Some(format!("{base_path}.metadata")),
        ));
    }
}

fn collect_event_validation_issues(
    event: &MemoryEvent,
    tenant: &TenantContext,
    index: usize,
    errors: &mut Vec<ImportValidationIssue>,
) {
    let base_path = format!("export.memory_events[{index}]");

    if event.org_id != tenant.org_id || event.user_id != tenant.user_id {
        errors.push(validation_issue(
            "EVENT_TENANT_MISMATCH",
            "memory event org_id/user_id does not match import tenant",
            Some(base_path.clone()),
        ));
    }

    if contains_forbidden_secret_fields(&event.metadata) {
        errors.push(validation_issue(
            "FORBIDDEN_SECRET_METADATA",
            "memory event metadata contains forbidden secret fields",
            Some(format!("{base_path}.metadata")),
        ));
    }
}

/// Validates export envelope and tenant alignment before import.
pub fn validate_export_for_import(
    export: &UserMemoryExport,
    tenant: &TenantContext,
) -> MemcoreResult<()> {
    let summary = collect_import_validation(export, tenant, false);
    if !summary.valid {
        return Err(MemcoreError::ValidationError(
            summary
                .first_error_message()
                .unwrap_or_else(|| "import validation failed".to_string()),
        ));
    }
    Ok(())
}

/// Validates a single exported fact for import.
pub fn validate_fact_for_import(fact: &Fact, tenant: &TenantContext) -> MemcoreResult<()> {
    let mut errors = Vec::new();
    collect_fact_validation_issues(fact, tenant, 0, &mut errors);
    if let Some(issue) = errors.into_iter().next() {
        return Err(MemcoreError::ValidationError(issue.message));
    }
    Ok(())
}

/// Validates a single exported memory event for import.
pub fn validate_event_for_import(event: &MemoryEvent, tenant: &TenantContext) -> MemcoreResult<()> {
    let mut errors = Vec::new();
    collect_event_validation_issues(event, tenant, 0, &mut errors);
    if let Some(issue) = errors.into_iter().next() {
        return Err(MemcoreError::ValidationError(issue.message));
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
pub fn resolve_import_fact_id(exported_id: Uuid, id_exists: bool, mode: super::ImportMode) -> Uuid {
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

    fn sample_export(facts: Vec<Fact>) -> UserMemoryExport {
        UserMemoryExport::new("org_a", "user_a", facts, vec![])
    }

    #[test]
    fn forbidden_metadata_keys_are_detected() {
        assert!(contains_forbidden_secret_fields(&json!({ "api_key": "x" })));
        assert!(contains_forbidden_secret_fields(
            &json!({ "nested": { "key_hash": "y" } })
        ));
        assert!(!contains_forbidden_secret_fields(
            &json!({ "source": "import" })
        ));
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

    #[test]
    fn collect_import_validation_reports_user_id_mismatch() {
        let export = UserMemoryExport::new("org_a", "user_b", vec![sample_fact()], vec![]);
        let summary = collect_import_validation(&export, &tenant(), false);
        assert!(!summary.valid);
        assert!(
            summary
                .errors
                .iter()
                .any(|issue| issue.code == "USER_ID_MISMATCH")
        );
    }

    #[test]
    fn collect_import_validation_reports_invalid_importance() {
        let mut fact = sample_fact();
        fact.importance = 2.0;
        let export = sample_export(vec![fact]);
        let summary = collect_import_validation(&export, &tenant(), false);
        assert!(!summary.valid);
        assert!(
            summary
                .errors
                .iter()
                .any(|issue| issue.code == "INVALID_IMPORTANCE")
        );
    }
}
