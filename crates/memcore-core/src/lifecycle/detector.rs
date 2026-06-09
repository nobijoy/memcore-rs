use memcore_common::MemcoreResult;

use crate::ports::{FactSearchQuery, FactStore};
use crate::{CandidateFact, Fact, TenantContext};

use super::types::RELATED_FACTS_SEARCH_LIMIT;

/// Finds existing facts that may conflict with a candidate using simple fact-store search.
pub async fn find_related_facts(
    fact_store: &dyn FactStore,
    tenant: &TenantContext,
    candidate: &CandidateFact,
) -> MemcoreResult<Vec<Fact>> {
    fact_store
        .search_facts(FactSearchQuery {
            tenant: tenant.clone(),
            memory_types: Some(vec![candidate.memory_type]),
            query_text: Some(candidate.content.clone()),
            limit: RELATED_FACTS_SEARCH_LIMIT,
            cursor: None,
            include_deleted: false,
        })
        .await
}
