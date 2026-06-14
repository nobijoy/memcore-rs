use chrono::{DateTime, Utc};
use memcore_core::{
    ListOrgUsersInput, ListOrgUsersOutput, OrgSummaryInput, OrgSummaryOutput, OrgUserSummary,
    DEFAULT_LIST_ORG_USERS_LIMIT, MAX_LIST_ORG_USERS_LIMIT,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub fn default_list_org_users_limit() -> usize {
    DEFAULT_LIST_ORG_USERS_LIMIT
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListOrgUsersQuery {
    #[serde(default = "default_list_org_users_limit")]
    pub limit: usize,
    pub cursor: Option<String>,
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
}
