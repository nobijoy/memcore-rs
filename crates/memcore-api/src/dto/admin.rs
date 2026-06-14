use chrono::{DateTime, Utc};
use memcore_core::{
    ListOrgUsersInput, ListOrgUsersOutput, MemoryEvent, OrgSummaryInput, OrgSummaryOutput,
    OrgUserSummary, SearchOrgMemoryEventsOutput, DEFAULT_LIST_ORG_USERS_LIMIT,
    DEFAULT_SEARCH_ORG_MEMORY_EVENTS_LIMIT, MAX_LIST_ORG_USERS_LIMIT,
    MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::memory_events::MemoryEventOperationResponse;

pub fn default_list_org_users_limit() -> usize {
    DEFAULT_LIST_ORG_USERS_LIMIT
}

pub fn default_search_org_memory_events_limit() -> usize {
    DEFAULT_SEARCH_ORG_MEMORY_EVENTS_LIMIT
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SearchOrgMemoryEventsQuery {
    pub user_id: Option<String>,
    pub fact_id: Option<String>,
    pub operation: Option<String>,
    #[serde(default = "default_search_org_memory_events_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListOrgUsersQuery {
    #[serde(default = "default_list_org_users_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchOrgMemoryEventsResponse {
    pub status: &'static str,
    pub events: Vec<AdminOrgMemoryEventItemResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdminOrgMemoryEventItemResponse {
    pub id: Uuid,
    pub user_id: String,
    pub fact_id: Option<Uuid>,
    pub operation: MemoryEventOperationResponse,
    pub previous_content: Option<String>,
    pub new_content: Option<String>,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgSummaryResponse {
    pub status: &'static str,
    pub summary: OrgSummaryBodyResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgSummaryBodyResponse {
    pub org_id: String,
    pub total_users: usize,
    pub total_facts: usize,
    pub total_events: Option<usize>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ListOrgUsersResponse {
    pub status: &'static str,
    pub users: Vec<OrgUserSummaryResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrgUserSummaryResponse {
    pub user_id: String,
    pub memory_count: usize,
    pub last_memory_at: Option<DateTime<Utc>>,
}

impl From<&MemoryEvent> for AdminOrgMemoryEventItemResponse {
    fn from(event: &MemoryEvent) -> Self {
        Self {
            id: event.id,
            user_id: event.user_id.clone(),
            fact_id: event.fact_id,
            operation: event.operation.into(),
            previous_content: event.previous_content.clone(),
            new_content: event.new_content.clone(),
            provider_name: event.provider_name.clone(),
            model_name: event.model_name.clone(),
            metadata: event.metadata.clone(),
            created_at: event.created_at,
        }
    }
}

impl From<OrgSummaryOutput> for OrgSummaryResponse {
    fn from(output: OrgSummaryOutput) -> Self {
        Self {
            status: "success",
            summary: OrgSummaryBodyResponse {
                org_id: output.org_id,
                total_users: output.total_users,
                total_facts: output.total_facts,
                total_events: output.total_events,
            },
        }
    }
}

impl From<OrgUserSummary> for OrgUserSummaryResponse {
    fn from(summary: OrgUserSummary) -> Self {
        Self {
            user_id: summary.user_id,
            memory_count: summary.memory_count,
            last_memory_at: summary.last_memory_at,
        }
    }
}

impl From<ListOrgUsersOutput> for ListOrgUsersResponse {
    fn from(output: ListOrgUsersOutput) -> Self {
        Self {
            status: "success",
            users: output.users.into_iter().map(OrgUserSummaryResponse::from).collect(),
            next_cursor: output.next_cursor,
        }
    }
}

impl From<SearchOrgMemoryEventsOutput> for SearchOrgMemoryEventsResponse {
    fn from(output: SearchOrgMemoryEventsOutput) -> Self {
        Self {
            status: "success",
            events: output
                .events
                .iter()
                .map(AdminOrgMemoryEventItemResponse::from)
                .collect(),
            next_cursor: output.next_cursor,
        }
    }
}

impl ListOrgUsersQuery {
    pub fn into_input(self, org_id: String) -> ListOrgUsersInput {
        ListOrgUsersInput {
            org_id,
            limit: self.limit,
            cursor: self.cursor,
        }
    }
}

pub fn org_summary_input(org_id: String) -> OrgSummaryInput {
    OrgSummaryInput { org_id }
}

pub fn validate_list_org_users_limit(limit: usize) -> Result<(), memcore_common::MemcoreError> {
    use memcore_common::MemcoreError;

    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > MAX_LIST_ORG_USERS_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_LIST_ORG_USERS_LIMIT}"
        )));
    }

    Ok(())
}

pub fn validate_search_org_memory_events_limit(
    limit: usize,
) -> Result<(), memcore_common::MemcoreError> {
    use memcore_common::MemcoreError;

    if limit == 0 {
        return Err(MemcoreError::ValidationError(
            "limit must be greater than 0".to_string(),
        ));
    }

    if limit > MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_SEARCH_ORG_MEMORY_EVENTS_LIMIT}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_org_users_limit_defaults_to_fifty() {
        let json = r#"{}"#;
        let query: ListOrgUsersQuery =
            serde_json::from_str(json).expect("deserialize list org users query");
        assert_eq!(query.limit, DEFAULT_LIST_ORG_USERS_LIMIT);
    }

    #[test]
    fn search_org_memory_events_limit_defaults_to_fifty() {
        let json = r#"{}"#;
        let query: SearchOrgMemoryEventsQuery =
            serde_json::from_str(json).expect("deserialize search org memory events query");
        assert_eq!(query.limit, DEFAULT_SEARCH_ORG_MEMORY_EVENTS_LIMIT);
    }
}
