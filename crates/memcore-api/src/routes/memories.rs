use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use memcore_common::MemcoreError;
use memcore_core::{AddMemoryInput, MemoryMessage, MessageRole, SearchMemoryInput, TenantContext};

use crate::dto::{
    AddMemoryRequest, AddMemoryResponse, MemoryMessageRequest, SearchMemoryRequest,
    SearchMemoryResponse,
};
use crate::routes::common::{ApiError, org_id_from_headers};
use crate::state::AppState;

pub async fn add_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AddMemoryRequest>,
) -> Result<Json<AddMemoryResponse>, ApiError> {
    let org_id = org_id_from_headers(&headers)?;
    validate_add_memory_request(&request)?;

    let tenant = TenantContext::new(org_id, request.user_id)?;
    let messages = map_messages(&request.messages)?;

    let output = state
        .memory_engine
        .add_memory(AddMemoryInput {
            tenant,
            messages,
            metadata: request.metadata,
        })
        .await?;

    Ok(Json(AddMemoryResponse::from(output)))
}

pub async fn search_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SearchMemoryRequest>,
) -> Result<Json<SearchMemoryResponse>, ApiError> {
    let org_id = org_id_from_headers(&headers)?;
    validate_search_memory_request(&request)?;

    let tenant = TenantContext::new(org_id, request.user_id)?;
    let memory_types = request.filters.parse_memory_types()?;

    let output = state
        .memory_engine
        .search_memory(SearchMemoryInput {
            tenant,
            query: request.query,
            limit: request.limit,
            memory_types,
            metadata_filter: None,
        })
        .await?;

    Ok(Json(SearchMemoryResponse::from(output)))
}

fn validate_add_memory_request(request: &AddMemoryRequest) -> Result<(), MemcoreError> {
    if request.user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        ));
    }

    if request.messages.is_empty() {
        return Err(MemcoreError::ValidationError(
            "messages cannot be empty".to_string(),
        ));
    }

    for message in &request.messages {
        if message.content.trim().is_empty() {
            return Err(MemcoreError::ValidationError(
                "message content cannot be empty".to_string(),
            ));
        }
    }

    Ok(())
}

fn validate_search_memory_request(request: &SearchMemoryRequest) -> Result<(), MemcoreError> {
    if request.user_id.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "user_id cannot be empty".to_string(),
        ));
    }

    if request.query.trim().is_empty() {
        return Err(MemcoreError::ValidationError(
            "query cannot be empty".to_string(),
        ));
    }

    if request.limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if request.limit > memcore_core::MAX_SEARCH_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {}",
            memcore_core::MAX_SEARCH_LIMIT
        )));
    }

    Ok(())
}

fn map_messages(messages: &[MemoryMessageRequest]) -> Result<Vec<MemoryMessage>, MemcoreError> {
    messages.iter().map(map_message).collect()
}

fn map_message(message: &MemoryMessageRequest) -> Result<MemoryMessage, MemcoreError> {
    let role = match message.role.trim().to_ascii_lowercase().as_str() {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        other => {
            return Err(MemcoreError::ValidationError(format!(
                "invalid message role: {other}"
            )));
        }
    };

    Ok(MemoryMessage {
        role,
        content: message.content.clone(),
    })
}
