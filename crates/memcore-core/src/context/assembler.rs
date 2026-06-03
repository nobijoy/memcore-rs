use crate::MemorySearchResult;

use super::types::EMPTY_CONTEXT_MESSAGE;

/// Formats search hits into a simple bullet-list context block for agents.
pub fn assemble_context(memories: &[MemorySearchResult], include_metadata: bool) -> String {
    if memories.is_empty() {
        return EMPTY_CONTEXT_MESSAGE.to_string();
    }

    let mut lines = vec!["Relevant long-term memories:".to_string()];

    for memory in memories {
        if include_metadata && !memory.metadata.is_null() && memory.metadata != serde_json::json!({})
        {
            lines.push(format!(
                "- {} (metadata: {})",
                memory.content,
                memory.metadata
            ));
        } else {
            lines.push(format!("- {}", memory.content));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::assemble_context;
    use crate::context::types::EMPTY_CONTEXT_MESSAGE;
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
}
