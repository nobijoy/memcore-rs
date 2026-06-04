use async_trait::async_trait;
use memcore_common::{MemcoreError, MemcoreResult};

use crate::traits::EmbeddingProvider;

use super::client::OpenAiClient;
use super::types::{EmbeddingsCreateRequest, EmbeddingsCreateResponse};

#[derive(Debug, Clone)]
pub struct OpenAiEmbeddingProvider {
    client: OpenAiClient,
    model: String,
    dimensions: usize,
}

impl OpenAiEmbeddingProvider {
    pub fn new(client: OpenAiClient, model: impl Into<String>, dimensions: usize) -> MemcoreResult<Self> {
        if dimensions == 0 {
            return Err(MemcoreError::ValidationError(
                "embedding dimensions must be greater than 0".to_string(),
            ));
        }

        Ok(Self {
            client,
            model: model.into(),
            dimensions,
        })
    }
}

fn validate_non_empty_text(text: &str) -> MemcoreResult<()> {
    if text.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "embedding text cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn validate_embedding_dimensions(embedding: &[f32], expected: usize) -> MemcoreResult<()> {
    if embedding.len() != expected {
        return Err(MemcoreError::ProviderError(format!(
            "OpenAI embedding dimension mismatch: expected {expected}, got {}",
            embedding.len()
        )));
    }
    Ok(())
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingProvider {
    async fn embed_text(&self, text: &str) -> MemcoreResult<Vec<f32>> {
        let mut batch = self.embed_batch(vec![text.to_string()]).await?;
        batch
            .pop()
            .ok_or_else(|| MemcoreError::ProviderError("OpenAI returned no embedding".to_string()))
    }

    async fn embed_batch(&self, texts: Vec<String>) -> MemcoreResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        for text in &texts {
            validate_non_empty_text(text)?;
        }

        let request = EmbeddingsCreateRequest {
            model: self.model.clone(),
            input: texts,
            dimensions: Some(self.dimensions),
        };

        let response: EmbeddingsCreateResponse = self.client.create_embeddings(&request).await?;

        if response.data.is_empty() {
            return Err(MemcoreError::ProviderError(
                "OpenAI embeddings response contained no data".to_string(),
            ));
        }

        let mut ordered = vec![Vec::new(); response.data.len()];
        for item in response.data {
            validate_embedding_dimensions(&item.embedding, self.dimensions)?;
            if item.index >= ordered.len() {
                return Err(MemcoreError::ProviderError(
                    "OpenAI embeddings response index out of range".to_string(),
                ));
            }
            ordered[item.index] = item.embedding;
        }

        if ordered.iter().any(Vec::is_empty) {
            return Err(MemcoreError::ProviderError(
                "OpenAI embeddings response missing one or more indices".to_string(),
            ));
        }

        Ok(ordered)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::EmbeddingProvider;

    #[test]
    fn zero_dimensions_rejected_at_construction() {
        let client = OpenAiClient::new("key", "https://api.openai.com/v1").expect("client");
        let error =
            OpenAiEmbeddingProvider::new(client, "text-embedding-3-small", 0).expect_err("fail");
        assert_eq!(
            error,
            MemcoreError::ValidationError(
                "embedding dimensions must be greater than 0".to_string()
            )
        );
    }

    #[tokio::test]
    async fn empty_text_returns_validation_error() {
        let client = OpenAiClient::new("key", "https://api.openai.com/v1").expect("client");
        let provider =
            OpenAiEmbeddingProvider::new(client, "text-embedding-3-small", 4).expect("provider");
        let error = provider.embed_text("   ").await.expect_err("should fail");
        assert_eq!(
            error,
            MemcoreError::ValidationError("embedding text cannot be empty".to_string())
        );
    }

    #[test]
    fn parses_embedding_response_and_validates_dimensions() {
        let payload = serde_json::json!({
            "data": [
                { "embedding": [0.1, 0.2, 0.3, 0.4], "index": 0 }
            ]
        });
        let response: EmbeddingsCreateResponse =
            serde_json::from_value(payload).expect("deserialize");
        validate_embedding_dimensions(&response.data[0].embedding, 4).expect("valid");
        let error = validate_embedding_dimensions(&response.data[0].embedding, 8).expect_err("bad");
        assert!(matches!(error, MemcoreError::ProviderError(_)));
    }
}
