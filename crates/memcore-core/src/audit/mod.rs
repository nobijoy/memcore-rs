use std::sync::Arc;

use memcore_common::MemcoreResult;
use serde_json::{json, Value};

use crate::ports::MemoryEventStore;
use crate::{Fact, FactOperationDecision, MemoryEvent, MemoryEventOperation, TenantContext};

/// Records an audit event when a store is configured. Failures are ignored (best-effort).
pub async fn record_event_best_effort(
    store: &Option<Arc<dyn MemoryEventStore>>,
    tenant: &TenantContext,
    event: MemoryEvent,
) {
    if let Some(store) = store.as_ref() {
        let _ = store.record_event(tenant, event).await;
    }
}

pub fn audit_metadata_for_decision(decision: &FactOperationDecision) -> Value {
    json!({
        "reason": decision.reason,
        "classification_confidence": decision.confidence,
        "target_fact_id": decision.target_fact_id,
    })
}

pub fn build_add_event(
    tenant: &TenantContext,
    fact: &Fact,
    provider_name: &Option<String>,
    model_name: &Option<String>,
    metadata: Value,
) -> MemoryEvent {
    MemoryEvent::new(
        tenant.org_id.clone(),
        tenant.user_id.clone(),
        Some(fact.id),
        MemoryEventOperation::Add,
        None,
        Some(fact.content.clone()),
        provider_name.clone(),
        model_name.clone(),
        metadata,
    )
}

pub fn build_update_event(
    tenant: &TenantContext,
    previous: &Fact,
    updated: &Fact,
    provider_name: &Option<String>,
    model_name: &Option<String>,
    metadata: Value,
) -> MemoryEvent {
    MemoryEvent::new(
        tenant.org_id.clone(),
        tenant.user_id.clone(),
        Some(updated.id),
        MemoryEventOperation::Update,
        Some(previous.content.clone()),
        Some(updated.content.clone()),
        provider_name.clone(),
        model_name.clone(),
        metadata,
    )
}

pub fn build_delete_event(
    tenant: &TenantContext,
    fact: &Fact,
    provider_name: &Option<String>,
    model_name: &Option<String>,
    metadata: Value,
) -> MemoryEvent {
    MemoryEvent::new(
        tenant.org_id.clone(),
        tenant.user_id.clone(),
        Some(fact.id),
        MemoryEventOperation::Delete,
        Some(fact.content.clone()),
        None,
        provider_name.clone(),
        model_name.clone(),
        metadata,
    )
}

pub fn build_noop_event(
    tenant: &TenantContext,
    decision: &FactOperationDecision,
    provider_name: &Option<String>,
    model_name: &Option<String>,
) -> MemoryEvent {
    MemoryEvent::new(
        tenant.org_id.clone(),
        tenant.user_id.clone(),
        decision.target_fact_id,
        MemoryEventOperation::NoOp,
        None,
        None,
        provider_name.clone(),
        model_name.clone(),
        audit_metadata_for_decision(decision),
    )
}

pub fn build_forget_user_event(
    tenant: &TenantContext,
    provider_name: &Option<String>,
    model_name: &Option<String>,
) -> MemoryEvent {
    MemoryEvent::new(
        tenant.org_id.clone(),
        tenant.user_id.clone(),
        None,
        MemoryEventOperation::ForgetUser,
        None,
        None,
        provider_name.clone(),
        model_name.clone(),
        json!({ "deleted": true }),
    )
}

/// Validates list query limits for memory audit events.
pub fn normalize_event_list_limit(limit: usize) -> MemcoreResult<usize> {
    use memcore_common::MemcoreError;
    use crate::ports::{DEFAULT_MEMORY_EVENT_LIST_LIMIT, MAX_MEMORY_EVENT_LIST_LIMIT};

    if limit == 0 {
        return Ok(DEFAULT_MEMORY_EVENT_LIST_LIMIT);
    }

    if limit > MAX_MEMORY_EVENT_LIST_LIMIT {
        return Err(MemcoreError::ValidationError(format!(
            "limit cannot exceed {MAX_MEMORY_EVENT_LIST_LIMIT}"
        )));
    }

    Ok(limit)
}
