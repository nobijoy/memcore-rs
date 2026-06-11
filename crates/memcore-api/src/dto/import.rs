use memcore_core::{
    ImportMode, ImportUserDataInput, ImportUserDataOutput, ImportValidationIssue,
    ImportValidationSummary, TenantContext, UserMemoryExport,
};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;

use super::export::{user_memory_export_from_response, UserMemoryExportResponse};

fn default_restore_events_false() -> bool {
    false
}

fn deserialize_import_export<'de, D>(deserializer: D) -> Result<UserMemoryExport, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if let Ok(response) = serde_json::from_value::<UserMemoryExportResponse>(value.clone()) {
        return user_memory_export_from_response(response).map_err(serde::de::Error::custom);
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ImportUserDataRequest {
    #[schema(value_type = UserMemoryExportResponse)]
    #[serde(deserialize_with = "deserialize_import_export")]
    pub export: UserMemoryExport,
    #[serde(default)]
    #[schema(value_type = String, example = "append")]
    pub mode: ImportMode,
    #[serde(default = "default_restore_events_false")]
    pub restore_events: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ImportUserDataResponse {
    pub status: &'static str,
    pub summary: ImportUserDataSummaryResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ImportUserDataSummaryResponse {
    pub imported_facts: usize,
    pub imported_events: usize,
    pub skipped_facts: usize,
    pub replaced_existing: bool,
    pub dry_run: bool,
    pub validation: ImportValidationSummaryResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ImportValidationSummaryResponse {
    pub valid: bool,
    pub errors: Vec<ImportValidationIssueResponse>,
    pub warnings: Vec<ImportValidationIssueResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ImportValidationIssueResponse {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

impl From<ImportValidationIssue> for ImportValidationIssueResponse {
    fn from(issue: ImportValidationIssue) -> Self {
        Self {
            code: issue.code,
            message: issue.message,
            path: issue.path,
        }
    }
}

impl From<ImportValidationSummary> for ImportValidationSummaryResponse {
    fn from(summary: ImportValidationSummary) -> Self {
        Self {
            valid: summary.valid,
            errors: summary.errors.into_iter().map(Into::into).collect(),
            warnings: summary.warnings.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ImportUserDataOutput> for ImportUserDataSummaryResponse {
    fn from(output: ImportUserDataOutput) -> Self {
        Self {
            imported_facts: output.imported_facts,
            imported_events: output.imported_events,
            skipped_facts: output.skipped_facts,
            replaced_existing: output.replaced_existing,
            dry_run: output.dry_run,
            validation: output.validation.into(),
        }
    }
}

impl ImportUserDataRequest {
    pub fn into_input(self, tenant: TenantContext) -> ImportUserDataInput {
        ImportUserDataInput {
            tenant,
            export: self.export,
            mode: self.mode,
            restore_events: self.restore_events,
            dry_run: self.dry_run,
        }
    }
}

impl From<ImportUserDataOutput> for ImportUserDataResponse {
    fn from(output: ImportUserDataOutput) -> Self {
        Self {
            status: "success",
            summary: ImportUserDataSummaryResponse::from(output),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memcore_core::USER_EXPORT_FORMAT_VERSION;

    #[test]
    fn import_response_serializes_summary() {
        let response = ImportUserDataResponse::from(ImportUserDataOutput {
            imported_facts: 2,
            imported_events: 1,
            skipped_facts: 0,
            replaced_existing: false,
            dry_run: false,
            validation: ImportValidationSummary::valid_empty(),
        });
        assert_eq!(response.status, "success");
        assert_eq!(response.summary.imported_facts, 2);
        assert!(!response.summary.dry_run);
        assert!(response.summary.validation.valid);
    }

    #[test]
    fn import_request_defaults_restore_events_false() {
        let json = format!(
            r#"{{
              "export": {{
                "format_version": "{USER_EXPORT_FORMAT_VERSION}",
                "org_id": "org_a",
                "user_id": "user_a",
                "exported_at": "2026-06-10T10:00:00Z",
                "facts": [],
                "memory_events": []
              }}
            }}"#
        );
        let request: ImportUserDataRequest =
            serde_json::from_str(&json).expect("deserialize import request");
        assert!(!request.restore_events);
        assert!(!request.dry_run);
        assert_eq!(request.mode, ImportMode::Append);
    }

    #[test]
    fn import_request_accepts_dry_run_flag() {
        let json = format!(
            r#"{{
              "export": {{
                "format_version": "{USER_EXPORT_FORMAT_VERSION}",
                "org_id": "org_a",
                "user_id": "user_a",
                "exported_at": "2026-06-10T10:00:00Z",
                "facts": [],
                "memory_events": []
              }},
              "dry_run": true
            }}"#
        );
        let request: ImportUserDataRequest =
            serde_json::from_str(&json).expect("deserialize dry_run request");
        assert!(request.dry_run);
    }

    #[test]
    fn import_request_accepts_api_export_fact_shape() {
        let json = r#"{
          "export": {
            "format_version": "memcore.user_export.v1",
            "org_id": "org_a",
            "user_id": "user_a",
            "exported_at": "2026-06-10T10:00:00Z",
            "facts": [{
              "id": "00000000-0000-4000-8000-000000000001",
              "org_id": "org_a",
              "user_id": "user_a",
              "content": "hello",
              "summary": null,
              "memory_type": "Profile",
              "source": "api_import",
              "confidence": 0.9,
              "importance": 0.8,
              "valid_at": null,
              "invalid_at": null,
              "recorded_at": "2026-06-10T10:00:00Z",
              "updated_at": "2026-06-10T10:00:00Z",
              "metadata": {}
            }],
            "memory_events": []
          }
        }"#;
        let request: ImportUserDataRequest =
            serde_json::from_str(json).expect("deserialize api-shaped export");
        assert_eq!(request.export.facts.len(), 1);
    }
}
