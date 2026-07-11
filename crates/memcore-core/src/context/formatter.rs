use chrono::{DateTime, Utc};
use memcore_common::MemcoreResult;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::MemorySearchResult;
use crate::MemoryType;

use super::format_options::{ContextFormat, ContextFormatOptions};
use super::types::EMPTY_CONTEXT_MESSAGE;

/// Memory item prepared for context formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMemoryItem {
    pub fact_id: Uuid,
    pub content: String,
    pub memory_type: MemoryType,
    pub score: f32,
    pub confidence: f32,
    pub importance: f32,
    pub valid_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

impl From<&MemorySearchResult> for ContextMemoryItem {
    fn from(result: &MemorySearchResult) -> Self {
        Self {
            fact_id: result.fact_id,
            content: result.content.clone(),
            memory_type: result.memory_type,
            score: result.score,
            confidence: result.confidence,
            importance: result.importance,
            valid_at: result.valid_at,
            metadata: result.metadata.clone(),
        }
    }
}

/// Rendered context output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedContext {
    pub context: String,
}

/// Formats ranked memory items into plain text, markdown, or JSON context strings.
pub struct ContextFormatter;

const SECTION_ORDER: [MemoryType; 8] = [
    MemoryType::Profile,
    MemoryType::Preference,
    MemoryType::Project,
    MemoryType::Skill,
    MemoryType::Task,
    MemoryType::Entity,
    MemoryType::Conversation,
    MemoryType::System,
];

impl ContextFormatter {
    pub fn format(
        memories: &[ContextMemoryItem],
        options: &ContextFormatOptions,
        legacy_include_fact_metadata: bool,
    ) -> MemcoreResult<FormattedContext> {
        if memories.is_empty() {
            return Ok(FormattedContext {
                context: EMPTY_CONTEXT_MESSAGE.to_string(),
            });
        }

        let context = match options.format {
            ContextFormat::PlainText => {
                Self::format_plain(memories, options, legacy_include_fact_metadata)
            }
            ContextFormat::Markdown => {
                Self::format_markdown(memories, options, legacy_include_fact_metadata)
            }
            ContextFormat::Json => {
                Self::format_json(memories, options, legacy_include_fact_metadata)?
            }
        };

        Ok(FormattedContext { context })
    }

    fn format_plain(
        memories: &[ContextMemoryItem],
        options: &ContextFormatOptions,
        legacy_include_fact_metadata: bool,
    ) -> String {
        if options.is_legacy_plain() && !legacy_include_fact_metadata {
            let mut lines = vec!["Relevant long-term memories:".to_string()];
            for memory in memories {
                lines.push(format!("- {}", memory.content));
            }
            return lines.join("\n");
        }

        if options.section_by_memory_type {
            let mut blocks = Vec::new();
            for memory_type in SECTION_ORDER {
                let section_items: Vec<_> = memories
                    .iter()
                    .filter(|memory| memory.memory_type == memory_type)
                    .collect();
                if section_items.is_empty() {
                    continue;
                }
                let mut lines = vec![format!("{}:", section_title(memory_type))];
                for memory in section_items {
                    lines.push(Self::format_plain_line(
                        memory,
                        options,
                        legacy_include_fact_metadata,
                    ));
                }
                blocks.push(lines.join("\n"));
            }
            return blocks.join("\n\n");
        }

        let mut lines: Vec<String> = memories
            .iter()
            .map(|memory| Self::format_plain_line(memory, options, legacy_include_fact_metadata))
            .collect();

        if options.is_legacy_plain() && legacy_include_fact_metadata {
            let mut with_header = vec!["Relevant long-term memories:".to_string()];
            with_header.append(&mut lines);
            return with_header.join("\n");
        }

        lines.join("\n")
    }

    fn format_plain_line(
        memory: &ContextMemoryItem,
        options: &ContextFormatOptions,
        legacy_include_fact_metadata: bool,
    ) -> String {
        let mut line = format!("- {}", memory.content);
        if legacy_include_fact_metadata
            && !memory.metadata.is_null()
            && memory.metadata != json!({})
        {
            line.push_str(&format!(" (metadata: {})", memory.metadata));
        }
        if let Some(suffix) = Self::plain_metadata_suffix(memory, options) {
            line.push_str(&suffix);
        }
        line
    }

    fn plain_metadata_suffix(
        memory: &ContextMemoryItem,
        options: &ContextFormatOptions,
    ) -> Option<String> {
        let parts = Self::metadata_parts(memory, options);
        if parts.is_empty() {
            return None;
        }
        Some(format!(" [{}]", parts.join(" ")))
    }

    fn format_markdown(
        memories: &[ContextMemoryItem],
        options: &ContextFormatOptions,
        legacy_include_fact_metadata: bool,
    ) -> String {
        if options.section_by_memory_type {
            let mut blocks = Vec::new();
            for memory_type in SECTION_ORDER {
                let section_items: Vec<_> = memories
                    .iter()
                    .filter(|memory| memory.memory_type == memory_type)
                    .collect();
                if section_items.is_empty() {
                    continue;
                }
                let mut lines = vec![format!("## {}", section_title(memory_type))];
                for memory in section_items {
                    lines.push(Self::format_markdown_line(
                        memory,
                        options,
                        legacy_include_fact_metadata,
                    ));
                }
                blocks.push(lines.join("\n"));
            }
            return blocks.join("\n\n");
        }

        memories
            .iter()
            .map(|memory| Self::format_markdown_line(memory, options, legacy_include_fact_metadata))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_markdown_line(
        memory: &ContextMemoryItem,
        options: &ContextFormatOptions,
        legacy_include_fact_metadata: bool,
    ) -> String {
        let mut line = format!("- {}", memory.content);
        if legacy_include_fact_metadata
            && !memory.metadata.is_null()
            && memory.metadata != json!({})
        {
            line.push_str(&format!(" (metadata: {})", memory.metadata));
        }
        if let Some(suffix) = Self::markdown_metadata_suffix(memory, options) {
            line.push_str(&suffix);
        }
        line
    }

    fn markdown_metadata_suffix(
        memory: &ContextMemoryItem,
        options: &ContextFormatOptions,
    ) -> Option<String> {
        let parts = Self::metadata_parts(memory, options);
        if parts.is_empty() {
            return None;
        }
        Some(format!(" _({})_", parts.join(", ")))
    }

    fn format_json(
        memories: &[ContextMemoryItem],
        options: &ContextFormatOptions,
        legacy_include_fact_metadata: bool,
    ) -> MemcoreResult<String> {
        let payload = if options.section_by_memory_type {
            let mut sections = Vec::new();
            for memory_type in SECTION_ORDER {
                let section_items: Vec<_> = memories
                    .iter()
                    .filter(|memory| memory.memory_type == memory_type)
                    .collect();
                if section_items.is_empty() {
                    continue;
                }
                sections.push(json!({
                    "type": section_title(memory_type),
                    "memories": section_items
                        .iter()
                        .map(|memory| Self::memory_json_object(memory, options, legacy_include_fact_metadata))
                        .collect::<Vec<_>>(),
                }));
            }
            json!({ "sections": sections })
        } else {
            json!({
                "memories": memories
                    .iter()
                    .map(|memory| Self::memory_json_object(memory, options, legacy_include_fact_metadata))
                    .collect::<Vec<_>>(),
            })
        };

        Ok(serde_json::to_string(&payload).expect("context json serialization"))
    }

    fn memory_json_object(
        memory: &ContextMemoryItem,
        options: &ContextFormatOptions,
        legacy_include_fact_metadata: bool,
    ) -> Value {
        let mut object = json!({ "content": memory.content });

        if let Some(map) = object.as_object_mut() {
            if options.include_memory_ids {
                map.insert("memory_id".to_string(), json!(memory.fact_id.to_string()));
            }
            if options.include_memory_types {
                map.insert(
                    "memory_type".to_string(),
                    json!(memory_type_label(memory.memory_type)),
                );
            }
            if options.include_scores {
                map.insert("score".to_string(), json!(memory.score));
            }
            if options.include_timestamps
                && let Some(timestamp) = memory.valid_at
            {
                map.insert("valid_at".to_string(), json!(timestamp.to_rfc3339()));
            }
            if options.include_confidence {
                map.insert("confidence".to_string(), json!(memory.confidence));
            }
            if options.include_importance {
                map.insert("importance".to_string(), json!(memory.importance));
            }
            if legacy_include_fact_metadata
                && !memory.metadata.is_null()
                && memory.metadata != json!({})
            {
                map.insert("metadata".to_string(), memory.metadata.clone());
            }
        }

        object
    }

    fn metadata_parts(memory: &ContextMemoryItem, options: &ContextFormatOptions) -> Vec<String> {
        let mut parts = Vec::new();
        if options.include_memory_ids {
            parts.push(format!("id={}", memory.fact_id));
        }
        if options.include_memory_types {
            parts.push(format!("type={}", memory_type_label(memory.memory_type)));
        }
        if options.include_scores {
            parts.push(format!("score={:.2}", memory.score));
        }
        if options.include_timestamps
            && let Some(timestamp) = memory.valid_at
        {
            parts.push(format!("valid_at={}", timestamp.to_rfc3339()));
        }
        if options.include_confidence {
            parts.push(format!("confidence={:.2}", memory.confidence));
        }
        if options.include_importance {
            parts.push(format!("importance={:.2}", memory.importance));
        }
        parts
    }
}

pub fn section_title(memory_type: MemoryType) -> &'static str {
    match memory_type {
        MemoryType::Profile => "Profile",
        MemoryType::Preference => "Preferences",
        MemoryType::Project => "Projects",
        MemoryType::Skill => "Skills",
        MemoryType::Task => "Tasks",
        MemoryType::Entity => "Entities",
        MemoryType::Conversation => "Conversation",
        MemoryType::System => "System",
    }
}

pub fn memory_type_label(memory_type: MemoryType) -> &'static str {
    match memory_type {
        MemoryType::Profile => "Profile",
        MemoryType::Preference => "Preference",
        MemoryType::Project => "Project",
        MemoryType::Skill => "Skill",
        MemoryType::Task => "Task",
        MemoryType::Entity => "Entity",
        MemoryType::Conversation => "Conversation",
        MemoryType::System => "System",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_item(content: &str, memory_type: MemoryType, score: f32) -> ContextMemoryItem {
        ContextMemoryItem {
            fact_id: Uuid::new_v4(),
            content: content.to_string(),
            memory_type,
            score,
            confidence: 0.85,
            importance: 0.9,
            valid_at: Some(Utc::now()),
            metadata: json!({}),
        }
    }

    #[test]
    fn markdown_format_without_sections() {
        let items = vec![
            sample_item("User is building memcore.", MemoryType::Project, 0.9),
            sample_item(
                "User prefers concise explanations.",
                MemoryType::Preference,
                0.8,
            ),
        ];
        let options = ContextFormatOptions {
            format: ContextFormat::Markdown,
            ..ContextFormatOptions::default()
        };

        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        assert!(formatted.context.contains("- User is building memcore."));
        assert!(!formatted.context.contains("## "));
    }

    #[test]
    fn markdown_format_with_sections() {
        let items = vec![
            sample_item("User is a full-stack developer.", MemoryType::Profile, 0.9),
            sample_item(
                "User prefers concise technical explanations.",
                MemoryType::Preference,
                0.8,
            ),
            sample_item("User is building memcore.", MemoryType::Project, 0.7),
        ];
        let options = ContextFormatOptions::structured_markdown();

        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        let profile_pos = formatted.context.find("## Profile").unwrap();
        let preferences_pos = formatted.context.find("## Preferences").unwrap();
        let projects_pos = formatted.context.find("## Projects").unwrap();
        assert!(profile_pos < preferences_pos);
        assert!(preferences_pos < projects_pos);
    }

    #[test]
    fn plain_text_format_with_sections() {
        let items = vec![sample_item(
            "User is a developer.",
            MemoryType::Profile,
            0.9,
        )];
        let options = ContextFormatOptions {
            format: ContextFormat::PlainText,
            section_by_memory_type: true,
            ..ContextFormatOptions::default()
        };

        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        assert!(formatted.context.starts_with("Profile:\n- "));
    }

    #[test]
    fn json_format_without_sections() {
        let items = vec![sample_item(
            "User is building memcore.",
            MemoryType::Project,
            0.82,
        )];
        let options = ContextFormatOptions {
            format: ContextFormat::Json,
            ..ContextFormatOptions::default()
        };

        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        let value: Value = serde_json::from_str(&formatted.context).unwrap();
        assert_eq!(value["memories"][0]["content"], "User is building memcore.");
    }

    #[test]
    fn json_format_with_sections() {
        let items = vec![sample_item(
            "User is a developer.",
            MemoryType::Profile,
            0.9,
        )];
        let options = ContextFormatOptions {
            format: ContextFormat::Json,
            section_by_memory_type: true,
            ..ContextFormatOptions::default()
        };

        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        let value: Value = serde_json::from_str(&formatted.context).unwrap();
        assert_eq!(value["sections"][0]["type"], "Profile");
    }

    #[test]
    fn ranked_order_preserved_inside_section() {
        let items = vec![
            sample_item("first project", MemoryType::Project, 0.9),
            sample_item("second project", MemoryType::Project, 0.8),
        ];
        let options = ContextFormatOptions::structured_markdown();
        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        let first = formatted.context.find("first project").unwrap();
        let second = formatted.context.find("second project").unwrap();
        assert!(first < second);
    }

    #[test]
    fn metadata_excluded_by_default() {
        let items = vec![sample_item("content", MemoryType::Skill, 0.5)];
        let formatted =
            ContextFormatter::format(&items, &ContextFormatOptions::default(), false).unwrap();
        assert!(!formatted.context.contains("score"));
        assert!(!formatted.context.contains("memory_id"));
    }

    #[test]
    fn memory_ids_included_only_when_requested() {
        let item = sample_item("content", MemoryType::Skill, 0.5);
        let fact_id = item.fact_id;
        let options = ContextFormatOptions {
            include_memory_ids: true,
            ..ContextFormatOptions::default()
        };
        let formatted = ContextFormatter::format(&[item], &options, false).unwrap();
        assert!(formatted.context.contains(&fact_id.to_string()));
    }

    #[test]
    fn scores_included_only_when_requested() {
        let items = vec![sample_item("content", MemoryType::Project, 0.82)];
        let options = ContextFormatOptions {
            format: ContextFormat::Markdown,
            include_scores: true,
            include_memory_types: true,
            ..ContextFormatOptions::default()
        };
        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        assert!(formatted.context.contains("score=0.82") || formatted.context.contains("0.82"));
    }

    #[test]
    fn timestamps_included_only_when_requested() {
        let items = vec![sample_item("content", MemoryType::Skill, 0.5)];
        let options = ContextFormatOptions {
            include_timestamps: true,
            ..ContextFormatOptions::default()
        };
        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        assert!(formatted.context.contains("valid_at="));
    }

    #[test]
    fn confidence_and_importance_included_only_when_requested() {
        let items = vec![sample_item("content", MemoryType::Skill, 0.5)];
        let options = ContextFormatOptions {
            include_confidence: true,
            include_importance: true,
            ..ContextFormatOptions::default()
        };
        let formatted = ContextFormatter::format(&items, &options, false).unwrap();
        assert!(formatted.context.contains("confidence="));
        assert!(formatted.context.contains("importance="));
    }

    #[test]
    fn no_sensitive_fields_in_formatted_output_by_default() {
        let mut item = sample_item("safe content", MemoryType::System, 0.5);
        item.metadata = json!({
            "input_text": "secret user message",
            "api_key": "mc_live_secret",
            "key_hash": "deadbeef"
        });
        let options = ContextFormatOptions {
            format: ContextFormat::Json,
            ..ContextFormatOptions::default()
        };
        let formatted = ContextFormatter::format(&[item], &options, false).unwrap();
        assert!(!formatted.context.contains("mc_live_secret"));
        assert!(!formatted.context.contains("secret user message"));
        assert!(!formatted.context.contains("deadbeef"));
    }
}
