use serde::{Deserialize, Serialize};

use crate::{MemorySearchResult, MemoryType, TenantContext};

use super::budget::{ContextBudget, ContextBudgetUsage};
use super::format_options::ContextFormatOptions;

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
    #[serde(default)]
    pub format_options: ContextFormatOptions,
}

impl Default for BuildContextInput {
    fn default() -> Self {
        Self {
            tenant: TenantContext::new("org", "user").expect("tenant"),
            query: String::new(),
            max_memories: DEFAULT_CONTEXT_MAX_MEMORIES,
            memory_types: None,
            include_metadata: false,
            budget: ContextBudget::default(),
            format_options: ContextFormatOptions::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildContextOutput {
    pub context: String,
    pub memories: Vec<MemorySearchResult>,
    pub budget: ContextBudgetUsage,
}
