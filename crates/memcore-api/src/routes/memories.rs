use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use memcore_common::MemcoreError;
use memcore_core::{AddMemoryInput, MemoryMessage, MessageRole, TenantContext};

use crate::dto::{AddMemoryRequest, AddMemoryResponse, MemoryMessageRequest};
use crate::response::ErrorBody;
use crate::state::AppState;

const ORG_HEADER: &str = "X-Organization-ID";

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

fn org_id_from_headers(headers: &HeaderMap) -> Result<String, MemcoreError> {
    let value = headers
        .get(ORG_HEADER)
        .ok_or_else(|| {
            MemcoreError::ValidationError(format!("{ORG_HEADER} header is required"))
        })?
        .to_str()
        .map_err(|_| {
            MemcoreError::ValidationError(format!("{ORG_HEADER} header must be valid UTF-8"))
        })?;

    let org_id = value.trim();
    if org_id.is_empty() {
        return Err(MemcoreError::ValidationError(format!(
            "{ORG_HEADER} header is required"
        )));
    }

    Ok(org_id.to_string())
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

#[derive(Debug)]
pub struct ApiError((axum::http::StatusCode, Json<ErrorBody>));

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = self.0;
        (status, body).into_response()
    }
}

impl From<MemcoreError> for ApiError {
    fn from(error: MemcoreError) -> Self {
        let (status, body) = ErrorBody::from_memcore_error(error);
        Self((status, Json(body)))
    }
}
