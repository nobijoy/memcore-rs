use std::time::Duration;

use memcore_common::{MemcoreError, MemcoreResult};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::types::{ApiErrorResponse, ResponsesCreateRequest, ResponsesCreateResponse};

const DEFAULT_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl OpenAiClient {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> MemcoreResult<Self> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "OPENAI_API_KEY cannot be empty".to_string(),
            ));
        }

        let base_url = normalize_base_url(base_url.into());
        let http = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|err| {
                MemcoreError::Internal(format!("failed to build OpenAI HTTP client: {err}"))
            })?;

        Ok(Self {
            http,
            api_key,
            base_url,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn create_response(
        &self,
        request: &ResponsesCreateRequest,
    ) -> MemcoreResult<ResponsesCreateResponse> {
        self.post_json("/responses", request).await
    }

    pub async fn create_embeddings<T: Serialize, R: DeserializeOwned>(
        &self,
        request: &T,
    ) -> MemcoreResult<R> {
        self.post_json("/embeddings", request).await
    }

    pub(crate) fn responses_request_body(
        &self,
        model: &str,
        instructions: &str,
        input: Value,
        schema_name: &str,
        schema: Value,
    ) -> ResponsesCreateRequest {
        ResponsesCreateRequest {
            model: model.to_string(),
            input,
            instructions: Some(instructions.to_string()),
            text: Some(super::types::TextFormatConfig {
                format: super::types::TextFormat::JsonSchema {
                    name: schema_name.to_string(),
                    strict: true,
                    schema,
                },
            }),
        }
    }

    pub(crate) fn responses_text_request_body(
        &self,
        model: &str,
        instructions: &str,
        input: Value,
    ) -> ResponsesCreateRequest {
        ResponsesCreateRequest {
            model: model.to_string(),
            input,
            instructions: Some(instructions.to_string()),
            text: Some(super::types::TextFormatConfig {
                format: super::types::TextFormat::Text,
            }),
        }
    }

    async fn post_json<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> MemcoreResult<R> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await
            .map_err(map_transport_error)?;

        let status = response.status();
        let bytes = response.bytes().await.map_err(map_transport_error)?;

        if status.is_success() {
            return serde_json::from_slice(&bytes).map_err(|err| {
                MemcoreError::ProviderError(format!("invalid JSON from OpenAI: {err}"))
            });
        }

        Err(map_http_error(status, &bytes))
    }
}

fn normalize_base_url(base_url: String) -> String {
    let trimmed = base_url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return "https://api.openai.com/v1".to_string();
    }
    trimmed
}

fn map_transport_error(err: reqwest::Error) -> MemcoreError {
    if err.is_timeout() {
        return MemcoreError::ProviderError("OpenAI request timed out".to_string());
    }
    MemcoreError::ProviderError(format!("OpenAI HTTP request failed: {err}"))
}

fn map_http_error(status: StatusCode, body: &[u8]) -> MemcoreError {
    if status == StatusCode::UNAUTHORIZED {
        return MemcoreError::ProviderError("OpenAI API key is unauthorized".to_string());
    }

    if let Ok(parsed) = serde_json::from_slice::<ApiErrorResponse>(body) {
        return MemcoreError::ProviderError(format!(
            "OpenAI API error ({}): {}",
            status.as_u16(),
            parsed.error.message
        ));
    }

    MemcoreError::ProviderError(format!("OpenAI API error with status {}", status.as_u16()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_api_key_returns_validation_error() {
        let error = OpenAiClient::new("  ", "https://api.openai.com/v1").expect_err("should fail");
        assert_eq!(
            error,
            MemcoreError::ValidationError("OPENAI_API_KEY cannot be empty".to_string())
        );
    }

    #[test]
    fn responses_request_includes_json_schema_format() {
        let client = OpenAiClient::new("test-key", "https://api.openai.com/v1")
            .expect("client should build");
        let body = client.responses_request_body(
            "gpt-4.1-mini",
            "system instructions",
            json!([{"role":"user","content":"hello"}]),
            "fact_extraction",
            json!({"type":"object"}),
        );

        let serialized = serde_json::to_value(&body).expect("serialize");
        assert_eq!(serialized["model"], "gpt-4.1-mini");
        assert_eq!(serialized["text"]["format"]["type"], "json_schema");
        assert_eq!(serialized["text"]["format"]["name"], "fact_extraction");
        assert!(serialized["text"]["format"]["strict"].as_bool().unwrap());
    }
}
