mod client;
mod embeddings;
mod llm;
mod types;

pub use client::OpenAiClient;
pub use embeddings::OpenAiEmbeddingProvider;
pub use llm::OpenAiLlmProvider;

#[cfg(test)]
mod integration_tests {
    use memcore_core::{CandidateFact, Fact, MemorySource, MemoryType, TenantContext};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::inputs::{
        FactClassificationInput, FactExtractionInput, MemoryMessage, MessageRole,
        SummarizationInput,
    };
    use crate::traits::{EmbeddingProvider, LlmProvider};

    fn tenant() -> TenantContext {
        TenantContext::new("org_test", "user_test").expect("tenant")
    }

    #[tokio::test]
    async fn extract_facts_calls_responses_endpoint() {
        let server = MockServer::start().await;
        let response_body = json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "{\"facts\":[{\"content\":\"User likes Rust.\",\"memory_type\":\"Preference\",\"confidence\":0.9,\"importance\":0.8,\"valid_at\":null,\"metadata\":{}}]}"
                }]
            }]
        });

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&server)
            .await;

        let client = OpenAiClient::new("test-key", server.uri()).expect("client");
        let provider = OpenAiLlmProvider::new(client, "gpt-4.1-mini");

        let facts = provider
            .extract_facts(FactExtractionInput {
                tenant: tenant(),
                messages: vec![MemoryMessage {
                    role: MessageRole::User,
                    content: "I like Rust.".to_string(),
                }],
                metadata: json!({}),
            })
            .await
            .expect("extraction");

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "User likes Rust.");
    }

    #[tokio::test]
    async fn classify_fact_operation_parses_model_json() {
        let server = MockServer::start().await;
        let response_body = json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "{\"operation\":\"Add\",\"target_fact_id\":null,\"reason\":\"ok\",\"confidence\":0.85}"
                }]
            }]
        });

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&server)
            .await;

        let client = OpenAiClient::new("test-key", server.uri()).expect("client");
        let provider = OpenAiLlmProvider::new(client, "gpt-4.1-mini");

        let candidate = CandidateFact::new(
            "User likes Rust.",
            MemoryType::Preference,
            0.9,
            0.8,
            None,
            json!({}),
        )
        .expect("candidate");

        let decision = provider
            .classify_fact_operation(FactClassificationInput {
                tenant: tenant(),
                candidate_fact: candidate,
                existing_facts: vec![],
            })
            .await
            .expect("classification");

        assert_eq!(decision.operation, memcore_core::FactOperation::Add);
        assert_eq!(decision.confidence, 0.85);
    }

    #[tokio::test]
    async fn summarize_memory_returns_plain_text() {
        let server = MockServer::start().await;
        let response_body = json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "User prefers Rust for systems work."
                }]
            }]
        });

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&server)
            .await;

        let client = OpenAiClient::new("test-key", server.uri()).expect("client");
        let provider = OpenAiLlmProvider::new(client, "gpt-4.1-mini");

        let now = chrono::Utc::now();
        let fact = Fact::new(
            uuid::Uuid::new_v4(),
            "org_test",
            "user_test",
            MemoryType::Preference,
            "User likes Rust",
            None,
            MemorySource::UserMessage,
            0.9,
            0.8,
            None,
            None,
            now,
            now,
            json!({}),
        )
        .expect("fact");

        let summary = provider
            .summarize_memory(SummarizationInput {
                tenant: tenant(),
                facts: vec![fact],
                max_tokens: None,
            })
            .await
            .expect("summary");

        assert_eq!(summary, "User prefers Rust for systems work.");
    }

    #[tokio::test]
    async fn embed_batch_calls_embeddings_endpoint() {
        let server = MockServer::start().await;
        let response_body = json!({
            "data": [
                { "embedding": [0.1, 0.2, 0.3, 0.4], "index": 0 },
                { "embedding": [0.5, 0.6, 0.7, 0.8], "index": 1 }
            ]
        });

        Mock::given(method("POST"))
            .and(path("/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&server)
            .await;

        let client = OpenAiClient::new("test-key", server.uri()).expect("client");
        let provider =
            OpenAiEmbeddingProvider::new(client, "text-embedding-3-small", 4).expect("provider");

        let embeddings = provider
            .embed_batch(vec!["alpha".to_string(), "beta".to_string()])
            .await
            .expect("embeddings");

        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].len(), 4);
    }

    #[tokio::test]
    async fn unauthorized_api_key_maps_to_provider_error() {
        let server = MockServer::start().await;
        let response_body = json!({
            "error": {
                "message": "Incorrect API key provided",
                "type": "invalid_request_error"
            }
        });

        Mock::given(method("POST"))
            .and(path("/embeddings"))
            .respond_with(ResponseTemplate::new(401).set_body_json(response_body))
            .mount(&server)
            .await;

        let client = OpenAiClient::new("bad-key", server.uri()).expect("client");
        let provider =
            OpenAiEmbeddingProvider::new(client, "text-embedding-3-small", 4).expect("provider");

        let error = provider.embed_text("hello").await.expect_err("should fail");
        assert!(matches!(error, memcore_common::MemcoreError::ProviderError(_)));
        assert!(error.to_string().contains("unauthorized"));
    }

    #[tokio::test]
    #[ignore = "requires OPENAI_API_KEY; run with: cargo test -p memcore-providers openai_live -- --ignored --nocapture"]
    async fn openai_live_smoke_test() {
        let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
        let client = OpenAiClient::new(api_key, "https://api.openai.com/v1").expect("client");
        let provider =
            OpenAiEmbeddingProvider::new(client, "text-embedding-3-small", 256).expect("provider");
        let embedding = provider.embed_text("memcore smoke test").await.expect("embed");
        assert_eq!(embedding.len(), 256);
    }
}
