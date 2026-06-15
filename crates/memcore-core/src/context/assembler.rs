use crate::MemorySearchResult;

use super::budget::{ContextBudget, ContextBudgetUsage};
use super::token_estimator::{SimpleTokenEstimator, TokenEstimator};
use super::types::EMPTY_CONTEXT_MESSAGE;

/// Result of budget-aware context assembly.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembledContext {
    pub context: String,
    pub memories: Vec<MemorySearchResult>,
    pub budget: ContextBudgetUsage,
}

/// Formats search hits into a simple bullet-list context block for agents.
pub fn assemble_context(memories: &[MemorySearchResult], include_metadata: bool) -> String {
    assemble_context_with_budget(
        memories,
        include_metadata,
        &ContextBudget {
            max_tokens: usize::MAX / 4,
            reserved_tokens: 0,
        },
        &SimpleTokenEstimator,
    )
    .context
}

/// Assembles context from ranked memories while enforcing a token budget.
///
/// Memories are considered in ranked order. Each memory is included only if its
/// fully formatted line fits the remaining budget; oversized memories are skipped
/// without truncation, and later shorter memories may still be included.
pub fn assemble_context_with_budget(
    memories: &[MemorySearchResult],
    include_metadata: bool,
    budget: &ContextBudget,
    estimator: &impl TokenEstimator,
) -> AssembledContext {
    let available = budget.available_tokens();
    let total_candidates = memories.len();

    if memories.is_empty() {
        let context = EMPTY_CONTEXT_MESSAGE.to_string();
        return AssembledContext {
            context: context.clone(),
            memories: Vec::new(),
            budget: ContextBudgetUsage {
                max_tokens: budget.max_tokens,
                reserved_tokens: budget.reserved_tokens,
                available_tokens: available,
                used_tokens: estimator.estimate_tokens(&context),
                included_memories: 0,
                skipped_memories: 0,
            },
        };
    }

    let header = "Relevant long-term memories:";
    let header_tokens = estimator.estimate_tokens(header);
    let newline_tokens = 1;

    let mut included = Vec::new();
    let mut skipped = 0;
    let mut used = 0;

    for memory in memories {
        let line = format_memory_line(memory, include_metadata);
        let line_tokens = estimator.estimate_tokens(&line);

        let additional = if included.is_empty() {
            header_tokens + newline_tokens + line_tokens
        } else {
            newline_tokens + line_tokens
        };

        if used + additional <= available {
            used += additional;
            included.push(memory.clone());
        } else {
            skipped += 1;
        }
    }

    if included.is_empty() {
        let context = EMPTY_CONTEXT_MESSAGE.to_string();
        return AssembledContext {
            context: context.clone(),
            memories: Vec::new(),
            budget: ContextBudgetUsage {
                max_tokens: budget.max_tokens,
                reserved_tokens: budget.reserved_tokens,
                available_tokens: available,
                used_tokens: estimator.estimate_tokens(&context),
                included_memories: 0,
                skipped_memories: total_candidates,
            },
        };
    }

    let context = format_context_from_memories(&included, include_metadata);
    let used_tokens = estimator.estimate_tokens(&context).min(available);

    AssembledContext {
        context,
        memories: included.clone(),
        budget: ContextBudgetUsage {
            max_tokens: budget.max_tokens,
            reserved_tokens: budget.reserved_tokens,
            available_tokens: available,
            used_tokens,
            included_memories: included.len(),
            skipped_memories: skipped,
        },
    }
}

fn format_memory_line(memory: &MemorySearchResult, include_metadata: bool) -> String {
    if include_metadata && !memory.metadata.is_null() && memory.metadata != serde_json::json!({}) {
        format!("- {} (metadata: {})", memory.content, memory.metadata)
    } else {
        format!("- {}", memory.content)
    }
}

fn format_context_from_memories(memories: &[MemorySearchResult], include_metadata: bool) -> String {
    let mut lines = vec!["Relevant long-term memories:".to_string()];
    for memory in memories {
        lines.push(format_memory_line(memory, include_metadata));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::context::budget::{ContextBudget, DEFAULT_CONTEXT_RESERVED_TOKENS};
    use crate::{MemorySearchResult, MemoryType};

    fn sample_result(content: &str) -> MemorySearchResult {
        MemorySearchResult {
            fact_id: Uuid::new_v4(),
            content: content.to_string(),
            memory_type: MemoryType::Skill,
            score: 0.9,
            confidence: 0.8,
            importance: 0.7,
            valid_at: None,
            metadata: json!({}),
        }
    }

    #[test]
    fn empty_memories_returns_safe_message() {
        assert_eq!(assemble_context(&[], false), EMPTY_CONTEXT_MESSAGE);
    }

    #[test]
    fn formats_bullet_list_without_metadata() {
        let context = assemble_context(
            &[
                sample_result("User is learning Rust."),
                sample_result("User is building a memory engine."),
            ],
            false,
        );

        assert!(context.starts_with("Relevant long-term memories:"));
        assert!(context.contains("- User is learning Rust."));
        assert!(context.contains("- User is building a memory engine."));
    }

    #[test]
    fn includes_metadata_when_requested() {
        let mut result = sample_result("Tagged memory");
        result.metadata = json!({ "tag": "work" });

        let context = assemble_context(&[result], true);
        assert!(context.contains("metadata:"));
        assert!(context.contains("Tagged memory"));
    }

    #[test]
    fn budget_includes_memories_that_fit() {
        let memories = vec![
            sample_result("Short memory one."),
            sample_result("Short memory two."),
        ];
        let budget = ContextBudget {
            max_tokens: 500,
            reserved_tokens: DEFAULT_CONTEXT_RESERVED_TOKENS,
        };

        let assembled =
            assemble_context_with_budget(&memories, false, &budget, &SimpleTokenEstimator);

        assert_eq!(assembled.memories.len(), 2);
        assert!(assembled.context.contains("Short memory one."));
        assert_eq!(assembled.budget.included_memories, 2);
        assert_eq!(assembled.budget.skipped_memories, 0);
    }

    #[test]
    fn budget_skips_memories_that_exceed_available_tokens() {
        let long_content = "x".repeat(4000);
        let memories = vec![
            sample_result(&long_content),
            sample_result("tiny"),
        ];
        let budget = ContextBudget {
            max_tokens: 120,
            reserved_tokens: 20,
        };

        let assembled =
            assemble_context_with_budget(&memories, false, &budget, &SimpleTokenEstimator);

        assert_eq!(assembled.memories.len(), 1);
        assert_eq!(assembled.memories[0].content, "tiny");
        assert_eq!(assembled.budget.included_memories, 1);
        assert_eq!(assembled.budget.skipped_memories, 1);
    }

    #[test]
    fn budget_preserves_ranked_order_for_included_memories() {
        let memories = vec![
            sample_result("first ranked"),
            sample_result(&"second very long memory ".repeat(200)),
            sample_result("third ranked"),
        ];
        let budget = ContextBudget {
            max_tokens: 200,
            reserved_tokens: 20,
        };

        let assembled =
            assemble_context_with_budget(&memories, false, &budget, &SimpleTokenEstimator);

        assert_eq!(assembled.memories.len(), 2);
        assert_eq!(assembled.memories[0].content, "first ranked");
        assert_eq!(assembled.memories[1].content, "third ranked");
    }

    #[test]
    fn used_tokens_never_exceeds_available_tokens() {
        let memories = vec![
            sample_result("alpha"),
            sample_result("beta"),
            sample_result("gamma"),
        ];
        let budget = ContextBudget {
            max_tokens: 80,
            reserved_tokens: 20,
        };

        let assembled =
            assemble_context_with_budget(&memories, false, &budget, &SimpleTokenEstimator);

        assert!(assembled.budget.used_tokens <= assembled.budget.available_tokens);
    }
}
