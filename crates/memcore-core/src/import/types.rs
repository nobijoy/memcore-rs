use serde::{Deserialize, Serialize};

use crate::export::UserMemoryExport;
use crate::TenantContext;

/// How imported facts are merged with existing user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    #[default]
    Append,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportUserDataInput {
    pub tenant: TenantContext,
    pub export: UserMemoryExport,
    pub mode: ImportMode,
    pub restore_events: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportValidationIssue {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportValidationSummary {
    pub valid: bool,
    pub errors: Vec<ImportValidationIssue>,
    pub warnings: Vec<ImportValidationIssue>,
}

impl ImportValidationSummary {
    pub fn valid_empty() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn first_error_message(&self) -> Option<String> {
        self.errors.first().map(|issue| issue.message.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportUserDataOutput {
    pub imported_facts: usize,
    pub imported_events: usize,
    pub skipped_facts: usize,
    pub replaced_existing: bool,
    pub dry_run: bool,
    pub validation: ImportValidationSummary,
}
