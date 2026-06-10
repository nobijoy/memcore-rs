use memcore_api::AppState;
use memcore_config::{
    EmbeddingProviderKind, LlmProviderKind, Settings, VectorBackend,
};
#[cfg(not(feature = "postgres"))]
use memcore_config::FactBackend;

#[tokio::test]
async fn mock_providers_start_without_openai_api_key() {
    let settings = Settings::default();

    AppState::initialize(settings)
        .await
        .expect("mock providers should start without OPENAI_API_KEY");
}

#[tokio::test]
async fn openai_llm_provider_starts_when_api_key_present() {
    let settings = Settings {
        llm_provider: LlmProviderKind::OpenAi,
        llm_model: "gpt-4.1-mini".to_string(),
        openai_api_key: Some("test-openai-key".to_string()),
        ..Settings::default()
    };

    AppState::initialize(settings)
        .await
        .expect("openai llm provider should initialize with api key");
}

#[tokio::test]
async fn openai_embedding_provider_starts_when_api_key_present() {
    let settings = Settings {
        embedding_provider: EmbeddingProviderKind::OpenAi,
        embedding_model: "text-embedding-3-small".to_string(),
        openai_api_key: Some("test-openai-key".to_string()),
        ..Settings::default()
    };

    AppState::initialize(settings)
        .await
        .expect("openai embedding provider should initialize with api key");
}

#[tokio::test]
async fn missing_openai_api_key_fails_when_openai_llm_selected() {
    let settings = Settings {
        llm_provider: LlmProviderKind::OpenAi,
        openai_api_key: None,
        ..Settings::default()
    };

    let error = match AppState::initialize(settings).await {
        Ok(_) => panic!("openai llm without key should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "validation_error");
    assert!(
        error
            .to_string()
            .contains("OPENAI_API_KEY is required when MEMCORE_LLM_PROVIDER=openai")
    );
}

#[tokio::test]
async fn missing_openai_api_key_fails_when_openai_embedding_selected() {
    let settings = Settings {
        embedding_provider: EmbeddingProviderKind::OpenAi,
        openai_api_key: None,
        ..Settings::default()
    };

    let error = match AppState::initialize(settings).await {
        Ok(_) => panic!("openai embedding without key should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "validation_error");
    assert!(
        error
            .to_string()
            .contains("OPENAI_API_KEY is required when MEMCORE_EMBEDDING_PROVIDER=openai")
    );
}

#[tokio::test]
async fn unsupported_openrouter_llm_provider_fails_startup() {
    let settings = Settings {
        llm_provider: LlmProviderKind::OpenRouter,
        ..Settings::default()
    };

    let error = match AppState::initialize(settings).await {
        Ok(_) => panic!("openrouter llm provider should not initialize"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "validation_error");
    assert!(
        error
            .to_string()
            .contains("openrouter LLM provider is not wired into the API yet")
    );
}

#[tokio::test]
async fn unsupported_anthropic_llm_provider_fails_startup() {
    let settings = Settings {
        llm_provider: LlmProviderKind::Anthropic,
        ..Settings::default()
    };

    let error = match AppState::initialize(settings).await {
        Ok(_) => panic!("anthropic llm provider should not initialize"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "validation_error");
    assert!(
        error
            .to_string()
            .contains("anthropic LLM provider is not wired into the API yet")
    );
}

#[tokio::test]
async fn sqlite_fact_backend_starts() {
    let settings = Settings::sqlite_memory();

    AppState::initialize(settings)
        .await
        .expect("sqlite fact backend should initialize");
}

#[tokio::test]
async fn mock_fact_backend_starts() {
    let settings = Settings::default();

    AppState::initialize(settings)
        .await
        .expect("mock fact backend should initialize");
}

#[cfg(not(feature = "postgres"))]
#[tokio::test]
async fn postgres_fact_backend_requires_postgres_feature() {
    let settings = Settings {
        fact_backend: FactBackend::Postgres,
        postgres_url: Some("postgres://localhost:5432/memcore".to_string()),
        ..Settings::default()
    };

    let error = match AppState::initialize(settings).await {
        Ok(_) => panic!("postgres backend should fail without postgres feature"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "validation_error");
    assert!(
        error
            .to_string()
            .contains("Postgres fact backend requires the `postgres` cargo feature")
    );
}

#[tokio::test]
async fn unsupported_qdrant_vector_backend_fails_startup() {
    let settings = Settings {
        vector_backend: VectorBackend::Qdrant,
        ..Settings::default()
    };

    let error = match AppState::initialize(settings).await {
        Ok(_) => panic!("qdrant vector backend should not initialize"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "validation_error");
    assert!(
        error
            .to_string()
            .contains("qdrant vector backend is not wired into the API yet")
    );
}
