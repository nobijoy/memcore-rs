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
    ChatCompletionMessage, ChatCompletionsRequest, classification_json_schema,
    extract_chat_completion_text, extract_output_text, fact_extraction_json_schema,
    parse_classification_response, parse_fact_extraction_response,
};

const FACT_EXTRACTION_INSTRUCTIONS: &str = r#"You extract durable long-term memory facts from conversation messages.
Rules:
- Extract only useful, durable facts worth remembering later.
- Do not store random short-lived details.
- Do not store sensitive data unless the user clearly asks for it.
- Prefer concise fact statements.
- confidence and importance must be floating-point numbers between 0.0 and 1.0 inclusive.
- memory_type must be one of: Preference, Profile, Goal, Relationship, Event, Knowledge, Instruction, Other.
- Return valid JSON only matching the schema.
- Do not include markdown or commentary."#;

const CLASSIFICATION_INSTRUCTIONS: &str = r#"You classify how a candidate memory fact should be stored relative to existing facts.
Return valid JSON only matching the schema.
Prefer Add when there is no clear conflict."#;

const SUMMARIZATION_INSTRUCTIONS: &str = r#"You summarize memory facts into concise plain text for downstream context.
Keep the summary short and factual.
Do not use markdown unless it materially improves clarity."#;

/// HTTP shape used for OpenAI-compatible LLM calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiLlmTransport {
    /// Native OpenAI `/responses` API (default for `api.openai.com`).
    Responses,
    /// OpenAI-compatible `/chat/completions` (Gemini, Groq, and most vendor proxies).
    ChatCompletions,
}

#[derive(Debug, Clone)]
pub struct OpenAiLlmProvider {
    client: OpenAiClient,
    model: String,
    transport: OpenAiLlmTransport,
}

impl OpenAiLlmProvider {
    /// Defaults to the OpenAI `/responses` transport (used by unit tests and direct OpenAI).
    pub fn new(client: OpenAiClient, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
            transport: OpenAiLlmTransport::Responses,
        }
    }

    /// Select transport from base URL (chat completions for Gemini/Groq-style OpenAI-compat hosts).
    pub fn for_base_url(client: OpenAiClient, model: impl Into<String>) -> Self {
        let transport = if OpenAiClient::prefers_chat_completions(client.base_url()) {
            OpenAiLlmTransport::ChatCompletions
        } else {
            OpenAiLlmTransport::Responses
        };
        Self {
            client,
            model: model.into(),
            transport,
        }
    }

    pub fn with_transport(mut self, transport: OpenAiLlmTransport) -> Self {
        self.transport = transport;
        self
    }

    pub fn transport(&self) -> OpenAiLlmTransport {
        self.transport
    }

    async fn request_json(
        &self,
        instructions: &str,
        input: serde_json::Value,
        schema_name: &str,
        schema: serde_json::Value,
    ) -> MemcoreResult<String> {
        match self.transport {
            OpenAiLlmTransport::Responses => {
                self.responses_json(instructions, input, schema_name, schema)
                    .await
            }
            OpenAiLlmTransport::ChatCompletions => {
                self.chat_json(instructions, input, schema).await
            }
        }
    }

    async fn request_plain_text(
        &self,
        instructions: &str,
        input: serde_json::Value,
    ) -> MemcoreResult<String> {
        match self.transport {
            OpenAiLlmTransport::Responses => self.responses_plain_text(instructions, input).await,
            OpenAiLlmTransport::ChatCompletions => self.chat_plain_text(instructions, input).await,
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

    fn chat_thinking_option(&self) -> Option<serde_json::Value> {
        if OpenAiClient::prefers_disabled_thinking(self.client.base_url()) {
            Some(json!({ "type": "disabled" }))
        } else {
            None
        }
    }

    async fn chat_json(
        &self,
        instructions: &str,
        input: serde_json::Value,
        schema: serde_json::Value,
    ) -> MemcoreResult<String> {
        let system = format!(
            "{instructions}\nReturn a single JSON object only that matches this JSON Schema:\n{schema}"
        );
        let request = ChatCompletionsRequest {
            model: self.model.clone(),
            messages: vec![
                ChatCompletionMessage {
                    role: "system".to_string(),
                    content: system,
                },
                ChatCompletionMessage {
                    role: "user".to_string(),
                    content: input.to_string(),
                },
            ],
            response_format: Some(json!({ "type": "json_object" })),
            max_tokens: Some(300),
            thinking: self.chat_thinking_option(),
        };
        let response = self.client.create_chat_completion(&request).await?;
        extract_chat_completion_text(&response)
    }

    async fn chat_plain_text(
        &self,
        instructions: &str,
        input: serde_json::Value,
    ) -> MemcoreResult<String> {
        let request = ChatCompletionsRequest {
            model: self.model.clone(),
            messages: vec![
                ChatCompletionMessage {
                    role: "system".to_string(),
                    content: instructions.to_string(),
                },
                ChatCompletionMessage {
                    role: "user".to_string(),
                    content: input.to_string(),
                },
            ],
            response_format: None,
            max_tokens: Some(300),
            thinking: self.chat_thinking_option(),
        };
        let response = self.client.create_chat_completion(&request).await?;
        extract_chat_completion_text(&response)
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
            .request_json(
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
            .request_json(
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
            .request_plain_text(SUMMARIZATION_INSTRUCTIONS, payload)
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
