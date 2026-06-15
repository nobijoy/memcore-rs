mod assembler;
mod budget;
mod cache;
mod compression;
mod compression_options;
mod format_options;
mod formatter;
mod summarizer;
mod token_estimator;
mod types;

pub use assembler::{
    apply_provider_compression_summary, assemble_context, assemble_context_with_budget,
    AssembledContext,
};
pub use cache::{
    build_context_cache_key, cached_entry_from_output, stable_sha256_hex, CachedContextEntry,
    ContextCache, ContextCacheConfig, ContextCacheKey, ContextCacheUsage,
    DEFAULT_CONTEXT_CACHE_MAX_ENTRIES, DEFAULT_CONTEXT_CACHE_TTL_SECONDS, InMemoryContextCache,
};
pub use budget::{
    ContextBudget, ContextBudgetUsage, DEFAULT_CONTEXT_MAX_TOKENS,
    DEFAULT_CONTEXT_RESERVED_TOKENS, MAX_CONTEXT_MAX_TOKENS,
};
pub use compression::{
    bullet_content, effective_summary_budget, format_summary_text, merge_context_with_summary,
    CompressedContext, SimpleContextCompressor,
};
pub use compression_options::{
    ContextCompressionMode, ContextCompressionOptions, ContextCompressionUsage,
    DEFAULT_SUMMARY_MAX_TOKENS, MAX_SUMMARY_MAX_TOKENS,
};
pub use format_options::{ContextFormat, ContextFormatOptions};
pub use formatter::{
    memory_type_label, section_title, ContextFormatter, ContextMemoryItem, FormattedContext,
};
pub use summarizer::{summarize_skipped_memories, ContextSummarizer, LlmContextSummarizer, SimpleContextSummarizer};
pub use token_estimator::{SimpleTokenEstimator, TokenEstimator};
pub use types::{
    BuildContextInput, BuildContextOutput, DEFAULT_CONTEXT_MAX_MEMORIES, EMPTY_CONTEXT_MESSAGE,
    MAX_CONTEXT_MAX_MEMORIES,
};
