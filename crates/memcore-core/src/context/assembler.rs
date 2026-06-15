use crate::MemorySearchResult;

use super::budget::{ContextBudget, ContextBudgetUsage};
use super::format_options::ContextFormatOptions;
use super::formatter::{ContextFormatter, ContextMemoryItem};
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
        &ContextFormatOptions::default(),
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
/// fully formatted representation (including section headings and metadata) fits
/// the remaining budget; oversized memories are skipped without truncation.
pub fn assemble_context_with_budget(
    memories: &[MemorySearchResult],
    legacy_include_fact_metadata: bool,
    format_options: &ContextFormatOptions,
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

    let items: Vec<ContextMemoryItem> = memories.iter().map(ContextMemoryItem::from).collect();
    let mut included_items: Vec<ContextMemoryItem> = Vec::new();
    let mut included_memories: Vec<MemorySearchResult> = Vec::new();
    let mut skipped = 0;

    for (item, memory) in items.iter().zip(memories.iter()) {
        let delta = token_delta_for_addition(
            &included_items,
            item,
            format_options,
            legacy_include_fact_metadata,
            estimator,
        );

        let used_so_far = if included_items.is_empty() {
            0
        } else {
            estimate_context_tokens(
                &included_items,
                format_options,
                legacy_include_fact_metadata,
                estimator,
            )
        };

        if used_so_far + delta <= available {
            included_items.push(item.clone());
            included_memories.push(memory.clone());
        } else {
            skipped += 1;
        }
    }

    if included_items.is_empty() {
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

    let context = ContextFormatter::format(
        &included_items,
        format_options,
        legacy_include_fact_metadata,
    )
    .expect("context formatting")
    .context;
    let used_tokens = estimator.estimate_tokens(&context).min(available);

    AssembledContext {
        context,
        memories: included_memories,
        budget: ContextBudgetUsage {
            max_tokens: budget.max_tokens,
            reserved_tokens: budget.reserved_tokens,
            available_tokens: available,
            used_tokens,
            included_memories: included_items.len(),
            skipped_memories: skipped,
        },
    }
}

fn estimate_context_tokens(
    items: &[ContextMemoryItem],
    format_options: &ContextFormatOptions,
    legacy_include_fact_metadata: bool,
    estimator: &impl TokenEstimator,
) -> usize {
    if items.is_empty() {
        return 0;
    }
    ContextFormatter::format(items, format_options, legacy_include_fact_metadata)
        .map(|formatted| estimator.estimate_tokens(&formatted.context))
        .unwrap_or(0)
}

fn token_delta_for_addition(
    current: &[ContextMemoryItem],
    candidate: &ContextMemoryItem,
    format_options: &ContextFormatOptions,
    legacy_include_fact_metadata: bool,
    estimator: &impl TokenEstimator,
) -> usize {
    let before = estimate_context_tokens(
        current,
        format_options,
        legacy_include_fact_metadata,
        estimator,
    );
    let mut with_candidate = current.to_vec();
    with_candidate.push(candidate.clone());
    let after = estimate_context_tokens(
        &with_candidate,
        format_options,
        legacy_include_fact_metadata,
        estimator,
    );
    after.saturating_sub(before)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::context::budget::{ContextBudget, DEFAULT_CONTEXT_RESERVED_TOKENS};
    use crate::context::format_options::ContextFormatOptions;
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

    fn sample_result_with_type(content: &str, memory_type: MemoryType) -> MemorySearchResult {
        MemorySearchResult {
            memory_type,
            ..sample_result(content)
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

        let assembled = assemble_context_with_budget(
            &memories,
            false,
            &ContextFormatOptions::default(),
            &budget,
            &SimpleTokenEstimator,
        );

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

        let assembled = assemble_context_with_budget(
            &memories,
            false,
            &ContextFormatOptions::default(),
            &budget,
            &SimpleTokenEstimator,
        );

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

        let assembled = assemble_context_with_budget(
            &memories,
            false,
            &ContextFormatOptions::default(),
            &budget,
            &SimpleTokenEstimator,
        );

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

        let assembled = assemble_context_with_budget(
            &memories,
            false,
            &ContextFormatOptions::default(),
            &budget,
            &SimpleTokenEstimator,
        );

        assert!(assembled.budget.used_tokens <= assembled.budget.available_tokens);
    }

    #[test]
    fn section_headings_count_toward_token_usage() {
        let memories = vec![
            sample_result_with_type("profile memory", MemoryType::Profile),
            sample_result_with_type("preference memory", MemoryType::Preference),
        ];
        let options = ContextFormatOptions::structured_markdown();
        let tight_budget = ContextBudget {
            max_tokens: 40,
            reserved_tokens: 10,
        };
        let loose_budget = ContextBudget {
            max_tokens: 500,
            reserved_tokens: 10,
        };

        let tight = assemble_context_with_budget(
            &memories,
            false,
            &options,
            &tight_budget,
            &SimpleTokenEstimator,
        );
        let loose = assemble_context_with_budget(
            &memories,
            false,
            &options,
            &loose_budget,
            &SimpleTokenEstimator,
        );

        assert!(tight.budget.included_memories <= loose.budget.included_memories);
        assert!(tight.budget.used_tokens <= tight.budget.available_tokens);
    }

    #[test]
    fn metadata_increases_formatted_output_size() {
        let memories = vec![sample_result(
            "short content with enough text to show metadata formatting differences",
        )];
        let without = ContextFormatOptions::default();
        let with_metadata = ContextFormatOptions {
            include_scores: true,
            include_memory_types: true,
            include_confidence: true,
            include_importance: true,
            ..ContextFormatOptions::default()
        };
        let budget = ContextBudget {
            max_tokens: 2000,
            reserved_tokens: 0,
        };

        let plain = assemble_context_with_budget(
            &memories,
            false,
            &without,
            &budget,
            &SimpleTokenEstimator,
        );
        let rich = assemble_context_with_budget(
            &memories,
            false,
            &with_metadata,
            &budget,
            &SimpleTokenEstimator,
        );

        assert!(rich.context.len() > plain.context.len());
        assert!(rich.budget.used_tokens >= plain.budget.used_tokens);
    }
}
