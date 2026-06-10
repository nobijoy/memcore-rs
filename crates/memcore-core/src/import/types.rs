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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportUserDataOutput {
    pub imported_facts: usize,
    pub imported_events: usize,
    pub skipped_facts: usize,
    pub replaced_existing: bool,
}
