use serde::{Deserialize, Serialize};

use crate::{MemorySearchResult, MemoryType, TenantContext};

use super::budget::{ContextBudget, ContextBudgetUsage};

/// Default maximum memories included in assembled context when callers omit a value.
pub const DEFAULT_CONTEXT_MAX_MEMORIES: usize = 10;

/// Maximum allowed memories for context assembly.
pub const MAX_CONTEXT_MAX_MEMORIES: usize = 20;

/// Context string returned when search finds no relevant memories.
pub const EMPTY_CONTEXT_MESSAGE: &str = "No relevant long-term memories found.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildContextInput {
    pub tenant: TenantContext,
    pub query: String,
    pub max_memories: usize,
    pub memory_types: Option<Vec<MemoryType>>,
    pub include_metadata: bool,
    #[serde(default)]
    pub budget: ContextBudget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildContextOutput {
    pub context: String,
    pub memories: Vec<MemorySearchResult>,
    pub budget: ContextBudgetUsage,
}
