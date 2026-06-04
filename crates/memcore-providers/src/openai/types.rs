use std::str::FromStr;

use memcore_common::{MemcoreError, MemcoreResult};
use memcore_core::{CandidateFact, FactOperation, FactOperationDecision, MemoryType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Parses memory type labels from model JSON (PascalCase or snake_case).
pub fn parse_memory_type_label(value: &str) -> MemcoreResult<MemoryType> {
    let normalized = value.trim().replace(' ', "_");
    let snake = to_snake_case(&normalized);

    match snake.as_str() {
        "profile" => Ok(MemoryType::Profile),
        "preference" => Ok(MemoryType::Preference),
        "project" => Ok(MemoryType::Project),
        "conversation" => Ok(MemoryType::Conversation),
        "task" => Ok(MemoryType::Task),
        "entity" => Ok(MemoryType::Entity),
        "skill" => Ok(MemoryType::Skill),
        "system" => Ok(MemoryType::System),
        _ => Err(MemcoreError::ValidationError(format!(
            "invalid memory_type from model: {value}"
        ))),
    }
}

/// Parses fact operation labels from model JSON (PascalCase or snake_case).
pub fn parse_fact_operation_label(value: &str) -> MemcoreResult<FactOperation> {
    let normalized = value.trim().replace(' ', "_");
    let snake = to_snake_case(&normalized);

    match snake.as_str() {
        "add" => Ok(FactOperation::Add),
        "update" => Ok(FactOperation::Update),
        "delete" => Ok(FactOperation::Delete),
        "no_op" | "noop" => Ok(FactOperation::NoOp),
        "archive" => Ok(FactOperation::Archive),
        "summarize" => Ok(FactOperation::Summarize),
        _ => Err(MemcoreError::ValidationError(format!(
            "invalid operation from model: {value}"
        ))),
    }
}

fn to_snake_case(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiErrorBody,
}

#[derive(Debug, Deserialize)]
pub struct ApiErrorBody {
    pub message: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub r#type: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ResponsesCreateRequest {
    pub model: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextFormatConfig>,
}

#[derive(Debug, Serialize)]
pub struct TextFormatConfig {
    pub format: TextFormat,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextFormat {
    #[serde(rename = "json_schema")]
    JsonSchema {
        name: String,
        strict: bool,
        schema: Value,
    },
    Text,
}

#[derive(Debug, Deserialize)]
pub struct ResponsesCreateResponse {
    pub status: Option<String>,
    #[serde(default)]
    pub output: Vec<ResponseOutputItem>,
    #[serde(default)]
    pub error: Option<ResponseError>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseError {
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseOutputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub content: Vec<ResponseOutputContent>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseOutputContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingsCreateRequest {
    pub model: String,
    pub input: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingsCreateResponse {
    pub data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingData {
    pub embedding: Vec<f32>,
    pub index: usize,
}

#[derive(Debug, Deserialize)]
pub struct FactExtractionModelResponse {
    pub facts: Vec<FactExtractionModelFact>,
}

#[derive(Debug, Deserialize)]
pub struct FactExtractionModelFact {
    pub content: String,
    pub memory_type: String,
    pub confidence: f32,
    pub importance: f32,
    #[serde(default)]
    pub valid_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct ClassificationModelResponse {
    pub operation: String,
    #[serde(default)]
    pub target_fact_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    pub confidence: f32,
}

pub fn fact_extraction_json_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "facts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "memory_type": { "type": "string" },
                        "confidence": { "type": "number" },
                        "importance": { "type": "number" },
                        "valid_at": { "type": ["string", "null"] },
                        "metadata": { "type": "object" }
                    },
                    "required": ["content", "memory_type", "confidence", "importance", "valid_at", "metadata"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["facts"],
        "additionalProperties": false
    })
}

pub fn classification_json_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "operation": { "type": "string" },
            "target_fact_id": { "type": ["string", "null"] },
            "reason": { "type": ["string", "null"] },
            "confidence": { "type": "number" }
        },
        "required": ["operation", "target_fact_id", "reason", "confidence"],
        "additionalProperties": false
    })
}

pub fn parse_fact_extraction_response(text: &str) -> MemcoreResult<Vec<CandidateFact>> {
    let parsed: FactExtractionModelResponse = serde_json::from_str(text).map_err(|err| {
        MemcoreError::ProviderError(format!("invalid fact extraction JSON from model: {err}"))
    })?;

    let mut candidates = Vec::new();
    for fact in parsed.facts {
        let memory_type = match parse_memory_type_label(&fact.memory_type) {
            Ok(value) => value,
            Err(_) => continue,
        };

        match CandidateFact::new(
            fact.content,
            memory_type,
            fact.confidence,
            fact.importance,
            fact.valid_at,
            fact.metadata,
        ) {
            Ok(candidate) => candidates.push(candidate),
            Err(_) => continue,
        }
    }

    Ok(candidates)
}

pub fn parse_classification_response(text: &str) -> MemcoreResult<FactOperationDecision> {
    let parsed: ClassificationModelResponse = serde_json::from_str(text).map_err(|err| {
        MemcoreError::ProviderError(format!("invalid classification JSON from model: {err}"))
    })?;

    let operation = parse_fact_operation_label(&parsed.operation)?;
    let target_fact_id = match parsed.target_fact_id {
        Some(id) if !id.trim().is_empty() => Some(Uuid::from_str(id.trim()).map_err(|_| {
            MemcoreError::ProviderError("invalid target_fact_id UUID from model".to_string())
        })?),
        _ => None,
    };

    if !(0.0..=1.0).contains(&parsed.confidence) {
        return Err(MemcoreError::ProviderError(
            "classification confidence must be between 0.0 and 1.0".to_string(),
        ));
    }

    Ok(FactOperationDecision {
        operation,
        target_fact_id,
        reason: parsed.reason,
        confidence: parsed.confidence,
    })
}

pub fn extract_output_text(response: &ResponsesCreateResponse) -> MemcoreResult<String> {
    if let Some(error) = &response.error {
        let message = error
            .message
            .clone()
            .unwrap_or_else(|| "OpenAI response error".to_string());
        return Err(MemcoreError::ProviderError(message));
    }

    let mut parts = Vec::new();
    for item in &response.output {
        if item.item_type != "message" {
            continue;
        }
        for content in &item.content {
            if content.content_type == "output_text" && !content.text.is_empty() {
                parts.push(content.text.as_str());
            }
        }
    }

    if parts.is_empty() {
        return Err(MemcoreError::ProviderError(
            "OpenAI response contained no output text".to_string(),
        ));
    }

    Ok(parts.join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use memcore_core::MemoryType;

    #[test]
    fn parses_fact_extraction_json() {
        let json = r#"{
            "facts": [
                {
                    "content": "User is learning Rust.",
                    "memory_type": "Skill",
                    "confidence": 0.95,
                    "importance": 0.82,
                    "valid_at": null,
                    "metadata": {}
                }
            ]
        }"#;

        let facts = parse_fact_extraction_response(json).expect("parse should succeed");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "User is learning Rust.");
        assert_eq!(facts[0].memory_type, MemoryType::Skill);
    }

    #[test]
    fn invalid_fact_extraction_json_returns_provider_error() {
        let error = parse_fact_extraction_response("not json").expect_err("should fail");
        assert!(matches!(error, MemcoreError::ProviderError(_)));
    }

    #[test]
    fn skips_invalid_candidate_facts() {
        let json = r#"{
            "facts": [
                {
                    "content": "",
                    "memory_type": "Skill",
                    "confidence": 0.95,
                    "importance": 0.82,
                    "valid_at": null,
                    "metadata": {}
                },
                {
                    "content": "Valid fact",
                    "memory_type": "conversation",
                    "confidence": 0.9,
                    "importance": 0.7,
                    "valid_at": null,
                    "metadata": {}
                }
            ]
        }"#;

        let facts = parse_fact_extraction_response(json).expect("parse should succeed");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "Valid fact");
    }

    #[test]
    fn parses_classification_json() {
        let json = r#"{
            "operation": "Add",
            "target_fact_id": null,
            "reason": "No conflict.",
            "confidence": 0.8
        }"#;

        let decision = parse_classification_response(json).expect("parse should succeed");
        assert_eq!(decision.operation, FactOperation::Add);
        assert_eq!(decision.confidence, 0.8);
    }

    #[test]
    fn extracts_output_text_from_response_payload() {
        let response: ResponsesCreateResponse = serde_json::from_value(serde_json::json!({
            "status": "completed",
            "output": [
                {
                    "type": "message",
                    "content": [
                        { "type": "output_text", "text": "{\"facts\":[]}" }
                    ]
                }
            ]
        }))
        .expect("fixture should deserialize");

        let text = extract_output_text(&response).expect("text should extract");
        assert_eq!(text, "{\"facts\":[]}");
    }
}
