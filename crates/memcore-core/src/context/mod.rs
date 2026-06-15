mod assembler;
mod budget;
mod format_options;
mod formatter;
mod token_estimator;
mod types;

pub use assembler::{assemble_context, assemble_context_with_budget, AssembledContext};
pub use budget::{
    ContextBudget, ContextBudgetUsage, DEFAULT_CONTEXT_MAX_TOKENS,
    DEFAULT_CONTEXT_RESERVED_TOKENS, MAX_CONTEXT_MAX_TOKENS,
};
pub use format_options::{ContextFormat, ContextFormatOptions};
pub use formatter::{memory_type_label, section_title, ContextFormatter, ContextMemoryItem, FormattedContext};
pub use token_estimator::{SimpleTokenEstimator, TokenEstimator};
pub use types::{
    BuildContextInput, BuildContextOutput, DEFAULT_CONTEXT_MAX_MEMORIES, EMPTY_CONTEXT_MESSAGE,
    MAX_CONTEXT_MAX_MEMORIES,
};
