use async_trait::async_trait;
use memcore_common::MemcoreResult;
use memcore_core::{CandidateFact, FactOperationDecision};
use serde_json::json;

use crate::inputs::{
    FactClassificationInput, FactExtractionInput, MemoryMessage, MessageRole, SummarizationInput,
};
use crate::traits::LlmProvider;

use super::client::OpenAiClient;
use super::types::{
    classification_json_schema, extract_output_text, fact_extraction_json_schema,
    parse_classification_response, parse_fact_extraction_response,
};

const FACT_EXTRACTION_INSTRUCTIONS: &str = r#"You extract durable long-term memory facts from conversation messages.
Rules:
- Extract only useful, durable facts worth remembering later.
- Do not store random short-lived details.
- Do not store sensitive data unless the user clearly asks for it.
- Prefer concise fact statements.
- Return valid JSON only matching the schema.
- Do not include markdown or commentary."#;

const CLASSIFICATION_INSTRUCTIONS: &str = r#"You classify how a candidate memory fact should be stored relative to existing facts.
Return valid JSON only matching the schema.
Prefer Add when there is no clear conflict."#;

const SUMMARIZATION_INSTRUCTIONS: &str = r#"You summarize memory facts into concise plain text for downstream context.
Keep the summary short and factual.
Do not use markdown unless it materially improves clarity."#;

#[derive(Debug, Clone)]
pub struct OpenAiLlmProvider {
    client: OpenAiClient,
    model: String,
}

impl OpenAiLlmProvider {
    pub fn new(client: OpenAiClient, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }

    async fn responses_json(
        &self,
        instructions: &str,
        input: serde_json::Value,
        schema_name: &str,
        schema: serde_json::Value,
    ) -> MemcoreResult<String> {
        let request = self.client.responses_request_body(
            &self.model,
            instructions,
            input,
            schema_name,
            schema,
        );
        let response = self.client.create_response(&request).await?;
        let text = extract_output_text(&response)?;
        Ok(text)
    }

    async fn responses_plain_text(
        &self,
        instructions: &str,
        input: serde_json::Value,
    ) -> MemcoreResult<String> {
        let request = self
            .client
            .responses_text_request_body(&self.model, instructions, input);
        let response = self.client.create_response(&request).await?;
        extract_output_text(&response)
    }
}

fn messages_to_input(messages: &[MemoryMessage]) -> serde_json::Value {
    let items: Vec<serde_json::Value> = messages
        .iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
            };
            json!({
                "role": role,
                "content": message.content,
            })
        })
        .collect();

    serde_json::Value::Array(items)
}

#[async_trait]
impl LlmProvider for OpenAiLlmProvider {
    async fn extract_facts(&self, input: FactExtractionInput) -> MemcoreResult<Vec<CandidateFact>> {
        let payload = json!({
            "tenant": {
                "org_id": input.tenant.org_id,
                "user_id": input.tenant.user_id,
            },
            "messages": messages_to_input(&input.messages),
            "metadata": input.metadata,
        });

        let text = self
            .responses_json(
                FACT_EXTRACTION_INSTRUCTIONS,
                payload,
                "memcore_fact_extraction",
                fact_extraction_json_schema(),
            )
            .await?;

        parse_fact_extraction_response(&text)
    }

    async fn classify_fact_operation(
        &self,
        input: FactClassificationInput,
    ) -> MemcoreResult<FactOperationDecision> {
        let existing: Vec<serde_json::Value> = input
            .existing_facts
            .iter()
            .map(|fact| {
                json!({
                    "id": fact.id,
                    "content": fact.content,
                    "memory_type": fact.memory_type,
                    "confidence": fact.confidence,
                    "importance": fact.importance,
                })
            })
            .collect();

        let payload = json!({
            "tenant": {
                "org_id": input.tenant.org_id,
                "user_id": input.tenant.user_id,
            },
            "candidate_fact": {
                "content": input.candidate_fact.content,
                "memory_type": input.candidate_fact.memory_type,
                "confidence": input.candidate_fact.confidence,
                "importance": input.candidate_fact.importance,
            },
            "existing_facts": existing,
        });

        let text = self
            .responses_json(
                CLASSIFICATION_INSTRUCTIONS,
                payload,
                "memcore_fact_classification",
                classification_json_schema(),
            )
            .await?;

        parse_classification_response(&text)
    }

    async fn summarize_memory(&self, input: SummarizationInput) -> MemcoreResult<String> {
        let facts: Vec<serde_json::Value> = input
            .facts
            .iter()
            .map(|fact| {
                json!({
                    "content": fact.content,
                    "memory_type": fact.memory_type,
                    "importance": fact.importance,
                })
            })
            .collect();

        let payload = json!({
            "tenant": {
                "org_id": input.tenant.org_id,
                "user_id": input.tenant.user_id,
            },
            "facts": facts,
            "max_tokens": input.max_tokens,
        });

        let summary = self
            .responses_plain_text(SUMMARIZATION_INSTRUCTIONS, payload)
            .await?;

        if let Some(max_tokens) = input.max_tokens {
            let max_chars = max_tokens.saturating_mul(4);
            if summary.len() > max_chars {
                return Ok(summary[..max_chars].to_string());
            }
        }

        Ok(summary)
    }
}
