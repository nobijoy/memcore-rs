use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEventOperation {
    Add,
    Update,
    Delete,
    NoOp,
    ForgetUser,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub id: Uuid,
    pub org_id: String,
    pub user_id: String,
    pub fact_id: Option<Uuid>,
    pub operation: MemoryEventOperation,
    /// Raw user input is not stored by default for privacy.
    pub input_text: Option<String>,
    pub previous_content: Option<String>,
    pub new_content: Option<String>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

impl MemoryEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        org_id: impl Into<String>,
        user_id: impl Into<String>,
        fact_id: Option<Uuid>,
        operation: MemoryEventOperation,
        previous_content: Option<String>,
        new_content: Option<String>,
        provider_name: Option<String>,
        model_name: Option<String>,
        metadata: Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            org_id: org_id.into(),
            user_id: user_id.into(),
            fact_id,
            operation,
            input_text: None,
            previous_content,
            new_content,
            provider_name,
            model_name,
            metadata,
            created_at: Utc::now(),
        }
    }
}
