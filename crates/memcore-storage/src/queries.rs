use memcore_core::{MemoryType, TenantContext};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactSearchQuery {
    pub tenant: TenantContext,
    pub memory_types: Option<Vec<MemoryType>>,
    pub query_text: Option<String>,
    pub limit: usize,
    pub cursor: Option<String>,
    pub include_deleted: bool,
}

impl FactSearchQuery {
    pub fn new(tenant: TenantContext, limit: usize) -> Self {
        Self {
            tenant,
            memory_types: None,
            query_text: None,
            limit,
            cursor: None,
            include_deleted: false,
        }
    }
}
