//! OpenAPI path definitions (documentation-only stubs; handlers live in `routes`).

#![allow(dead_code)]

use crate::dto::{
    AddMemoryRequest, AddMemoryResponse, BuildContextRequest, BuildContextResponse,
    CreateApiKeyRequest, CreateApiKeyResponse, DeleteMemoryResponse, ExportUserResponse,
    ApplyRetentionRequest, ApplyRetentionResponse, ForgetUserResponse, ImportUserDataRequest,
    ImportUserDataResponse, ListApiKeysResponse, ListOrgUsersResponse, OrgSummaryResponse,
    SearchOrgMemoryEventsResponse,
    ListMemoriesResponse, ListMemoryEventsResponse,
    RevokeApiKeyResponse, SearchMemoryRequest, SearchMemoryResponse,
};
use crate::response::ErrorBody;
use crate::routes::health::{HealthResponse, ReadyResponse};

/// Liveness probe.
#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses(
        (status = 200, description = "Service is alive", body = HealthResponse),
    )
)]
pub fn health() {}

/// Readiness probe with configured backend labels.
#[utoipa::path(
    get,
    path = "/ready",
    tag = "Health",
    responses(
        (status = 200, description = "Service is ready", body = ReadyResponse),
    )
)]
pub fn ready() {}

/// Minimal in-process Prometheus-compatible metrics (text/plain).
#[utoipa::path(
    get,
    path = "/metrics",
    tag = "Metrics",
    responses(
        (status = 200, description = "Prometheus text metrics when MEMCORE_METRICS_ENABLED=true", content_type = "text/plain", body = String),
        (status = 404, description = "Metrics endpoint disabled"),
    )
)]
pub fn metrics() {}

/// Extract and store memories from a user message conversation.
#[utoipa::path(
    post,
    path = "/api/v1/memories",
    tag = "Memories",
    request_body = AddMemoryRequest,
    params(
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Memories processed", body = AddMemoryResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn add_memory() {}

/// Semantic search over stored memories.
#[utoipa::path(
    post,
    path = "/api/v1/memories/search",
    tag = "Memories",
    request_body = SearchMemoryRequest,
    params(
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Search results", body = SearchMemoryResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn search_memory() {}

/// Build a formatted context string from relevant memories.
#[utoipa::path(
    post,
    path = "/api/v1/context",
    tag = "Context",
    request_body = BuildContextRequest,
    params(
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Context assembled", body = BuildContextResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn build_context() {}

/// List memories for a user. Cursor pagination is accepted but not fully implemented.
#[utoipa::path(
    get,
    path = "/api/v1/users/{user_id}/memories",
    tag = "Memories",
    params(
        ("user_id" = String, Path, description = "User identifier"),
        ("memory_type" = Option<String>, Query, description = "Filter by memory type (PascalCase label)"),
        ("q" = Option<String>, Query, description = "Case-insensitive keyword search over content and summary (max 200 chars)"),
        ("limit" = Option<usize>, Query, description = "Maximum results to return"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (accepted but ignored)"),
        ("include_deleted" = Option<bool>, Query, description = "Include soft-deleted memories"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "User memories", body = ListMemoriesResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn list_user_memories() {}

/// Soft-delete a single memory for a user.
#[utoipa::path(
    delete,
    path = "/api/v1/users/{user_id}/memories/{memory_id}",
    tag = "Memories",
    params(
        ("user_id" = String, Path, description = "User identifier"),
        ("memory_id" = String, Path, description = "Memory (fact) UUID"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Memory deleted", body = DeleteMemoryResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Memory not found", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn delete_user_memory() {}

/// Export memory facts and optional audit events for a user as JSON. Does not include API keys or event `input_text`.
#[utoipa::path(
    get,
    path = "/api/v1/users/{user_id}/export",
    tag = "Users",
    params(
        ("user_id" = String, Path, description = "User identifier"),
        ("include_events" = Option<bool>, Query, description = "Include memory audit events (default true)"),
        ("include_deleted" = Option<bool>, Query, description = "Include soft-deleted facts (default false)"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "User memory export", body = ExportUserResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing required scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn export_user_data() {}

/// Import user memory data from a `memcore.user_export.v1` JSON export. Set `dry_run=true` to validate without writes.
#[utoipa::path(
    post,
    path = "/api/v1/users/{user_id}/import",
    tag = "Users",
    params(
        ("user_id" = String, Path, description = "User identifier"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    request_body = ImportUserDataRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Import completed or dry-run validation summary", body = ImportUserDataResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing required scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn import_user_data() {}

/// Apply user-scoped retention cleanup for facts and audit events. `dry_run` defaults to `true`.
#[utoipa::path(
    post,
    path = "/api/v1/users/{user_id}/retention/apply",
    tag = "Users",
    params(
        ("user_id" = String, Path, description = "User identifier"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    request_body = ApplyRetentionRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Retention dry-run or apply summary", body = ApplyRetentionResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing required scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn apply_retention() {}

/// Delete all memories and vectors for a user within the organization.
#[utoipa::path(
    delete,
    path = "/api/v1/users/{user_id}",
    tag = "Users",
    params(
        ("user_id" = String, Path, description = "User identifier"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "User data forgotten", body = ForgetUserResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn forget_user() {}

/// List memory audit events for a user. `input_text` is not exposed. Cursor pagination is accepted but not fully implemented.
#[utoipa::path(
    get,
    path = "/api/v1/users/{user_id}/memory-events",
    tag = "Memory Events",
    params(
        ("user_id" = String, Path, description = "User identifier"),
        ("operation" = Option<String>, Query, description = "Filter by operation (PascalCase label)"),
        ("fact_id" = Option<String>, Query, description = "Filter by fact UUID"),
        ("created_after" = Option<String>, Query, description = "Inclusive RFC3339 lower bound on created_at"),
        ("created_before" = Option<String>, Query, description = "Exclusive RFC3339 upper bound on created_at"),
        ("q" = Option<String>, Query, description = "Case-insensitive keyword search over event fields (max 200 chars; does not search input_text)"),
        ("limit" = Option<usize>, Query, description = "Maximum results to return"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (accepted but ignored)"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Audit events", body = ListMemoryEventsResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn list_user_memory_events() {}

/// Create an API key for the organization. Returns the raw key once; it is never returned again in list responses.
#[utoipa::path(
    post,
    path = "/api/v1/api-keys",
    tag = "API Keys",
    request_body = CreateApiKeyRequest,
    params(
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "API key created (raw_key shown only in this response)", body = CreateApiKeyResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing AdminWrite scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn create_api_key() {}

/// List API keys for the organization. Does not include raw keys or key hashes.
#[utoipa::path(
    get,
    path = "/api/v1/api-keys",
    tag = "API Keys",
    params(
        ("include_revoked" = Option<bool>, Query, description = "Include revoked keys when true"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Organization API keys", body = ListApiKeysResponse),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing AdminRead or AdminWrite scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn list_api_keys() {}

/// Revoke an API key. Revoked keys can no longer authenticate.
#[utoipa::path(
    delete,
    path = "/api/v1/api-keys/{api_key_id}",
    tag = "API Keys",
    params(
        ("api_key_id" = String, Path, description = "API key UUID"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "API key revoked", body = RevokeApiKeyResponse),
        (status = 400, description = "Invalid api_key_id", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing AdminWrite scope in database auth mode", body = ErrorBody),
        (status = 404, description = "API key not found in organization", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn revoke_api_key() {}

/// Organization-level aggregate counts for admin visibility. Does not return memory content.
#[utoipa::path(
    get,
    path = "/api/v1/admin/org/summary",
    tag = "Admin",
    params(
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Organization summary", body = OrgSummaryResponse),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing AdminRead or AdminWrite scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn get_org_summary() {}

/// List users in the organization with memory aggregates. Does not return memory content.
#[utoipa::path(
    get,
    path = "/api/v1/admin/org/users",
    tag = "Admin",
    params(
        ("limit" = Option<usize>, Query, description = "Page size (default 50, max 100)"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (accepted but not implemented yet)"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Organization users", body = ListOrgUsersResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing AdminRead or AdminWrite scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn list_org_users() {}

/// Search memory audit events across the organization. Does not return input_text.
#[utoipa::path(
    get,
    path = "/api/v1/admin/org/memory-events",
    tag = "Admin",
    params(
        ("user_id" = Option<String>, Query, description = "Filter by user id"),
        ("fact_id" = Option<String>, Query, description = "Filter by fact UUID"),
        ("operation" = Option<String>, Query, description = "Filter by operation (Add, Update, Delete, NoOp, ForgetUser)"),
        ("created_after" = Option<String>, Query, description = "Inclusive RFC3339 lower bound on created_at"),
        ("created_before" = Option<String>, Query, description = "Exclusive RFC3339 upper bound on created_at"),
        ("q" = Option<String>, Query, description = "Case-insensitive keyword search over event fields and user_id (max 200 chars; does not search input_text)"),
        ("limit" = Option<usize>, Query, description = "Page size (default 50, max 100)"),
        ("cursor" = Option<String>, Query, description = "Pagination cursor (accepted but not implemented yet)"),
        ("X-Organization-ID" = String, Header, description = "Organization tenant id", example = "org_123"),
        ("X-Request-ID" = Option<String>, Header, description = "Optional request correlation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Organization memory audit events", body = SearchOrgMemoryEventsResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 401, description = "Missing or invalid API key", body = ErrorBody),
        (status = 403, description = "Missing AdminRead, AdminWrite, or AuditRead scope in database auth mode", body = ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = ErrorBody),
    )
)]
pub fn search_org_memory_events() {}
