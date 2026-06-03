use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Fact, MemorySearchResult, TenantContext};
use crate::ports::MemoryMessage;

/// Default minimum importance for add-memory filtering until config is wired into core.
pub const DEFAULT_MIN_IMPORTANCE: f32 = 0.55;

/// Default result limit for memory search when callers omit an explicit limit.
pub const DEFAULT_SEARCH_LIMIT: usize = 10;

/// Maximum allowed result limit for memory search.
pub const MAX_SEARCH_LIMIT: usize = 50;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchMemoryInput {
    pub tenant: TenantContext,
    pub query: String,
    pub limit: usize,
    pub memory_types: Option<Vec<crate::MemoryType>>,
    pub metadata_filter: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchMemoryOutput {
    pub results: Vec<MemorySearchResult>,
}
