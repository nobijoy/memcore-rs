use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Fact, MemoryEvent};

/// JSON export format version for per-user memory exports.
pub const USER_EXPORT_FORMAT_VERSION: &str = "memcore.user_export.v1";

/// Maximum facts included in a single user export (foundation phase).
pub const EXPORT_FACTS_LIMIT: usize = crate::MAX_LIST_MEMORIES_LIMIT;

/// Maximum memory events included in a single user export (foundation phase).
pub const EXPORT_EVENTS_LIMIT: usize = crate::MAX_LIST_MEMORY_EVENTS_LIMIT;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMemoryExport {
    pub org_id: String,
    pub user_id: String,
    pub exported_at: DateTime<Utc>,
    pub format_version: String,
    pub facts: Vec<Fact>,
    pub memory_events: Vec<MemoryEvent>,
}

impl UserMemoryExport {
    pub fn new(
        org_id: impl Into<String>,
        user_id: impl Into<String>,
        facts: Vec<Fact>,
        memory_events: Vec<MemoryEvent>,
    ) -> Self {
        Self {
            org_id: org_id.into(),
            user_id: user_id.into(),
            exported_at: Utc::now(),
            format_version: USER_EXPORT_FORMAT_VERSION.to_string(),
            facts,
            memory_events,
        }
    }
}
