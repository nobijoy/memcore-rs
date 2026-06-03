use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Fact, TenantContext};
use crate::ports::MemoryMessage;

/// Default minimum importance for add-memory filtering until config is wired into core.
pub const DEFAULT_MIN_IMPORTANCE: f32 = 0.55;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddMemoryInput {
    pub tenant: TenantContext,
    pub messages: Vec<MemoryMessage>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryOperationSummary {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub noop: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddMemoryOutput {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub noop: usize,
    pub memories: Vec<Fact>,
}

impl From<MemoryOperationSummary> for AddMemoryOutput {
    fn from(summary: MemoryOperationSummary) -> Self {
        Self {
            added: summary.added,
            updated: summary.updated,
            deleted: summary.deleted,
            noop: summary.noop,
            memories: Vec::new(),
        }
    }
}
