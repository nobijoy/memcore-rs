use serde::{Deserialize, Serialize};

use crate::ports::OrgUserSummary;

/// Default page size for listing organization users.
pub const DEFAULT_LIST_ORG_USERS_LIMIT: usize = 50;

/// Maximum page size for listing organization users.
pub const MAX_LIST_ORG_USERS_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgSummaryInput {
    pub org_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgSummaryOutput {
    pub org_id: String,
    pub total_users: usize,
    pub total_facts: usize,
    pub total_events: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOrgUsersInput {
    pub org_id: String,
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOrgUsersOutput {
    pub users: Vec<OrgUserSummary>,
    pub next_cursor: Option<String>,
}
