use memcore_api::AppState;
use memcore_config::{FactBackend, Settings, VectorBackend};

#[tokio::test]
async fn unsupported_postgres_fact_backend_fails_startup() {
    let settings = Settings {
        fact_backend: FactBackend::Postgres,
        postgres_url: Some("postgres://localhost:5432/memcore".to_string()),
        ..Settings::default()
    };

    let error = match AppState::initialize(settings).await {
        Ok(_) => panic!("postgres backend should not initialize"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "validation_error");
    assert!(
        error
            .to_string()
            .contains("postgres fact backend is not wired into the API yet")
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
