mod paths;

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::dto::{
    AddMemoryRequest, AddMemoryResponse, ApiKeyItemResponse, ApiKeyScopeResponse,
    BuildContextRequest, BuildContextResponse, ContextBudgetResponse, ContextCacheResponse,
    ContextCompressionResponse, ContextMemoryResponse, CreateApiKeyRequest,
    CreateApiKeyResponse, DeleteMemoryResponse, ExportFactItemResponse, ExportMemorySourceResponse,
    ApplyRetentionRequest, ApplyRetentionResponse, ApplyRetentionSummaryResponse, ExportUserResponse,
    ForgetUserResponse, ImportUserDataRequest, ImportUserDataResponse, ImportUserDataSummaryResponse,
    ImportValidationIssueResponse, ImportValidationSummaryResponse, ListApiKeysResponse,
    ListMemoriesResponse, ListOrgUsersResponse, OrgSummaryBodyResponse, OrgSummaryResponse,
    OrgUserSummaryResponse, SearchOrgMemoryEventsResponse, AdminOrgMemoryEventItemResponse,
    ListMemoryEventsResponse, ListMemoryItemResponse, MemoryEventItemResponse,
    MemoryEventOperationResponse, MemoryItemResponse, MemoryMessageRequest,
    MemoryOperationSummaryResponse, RevokeApiKeyResponse, SearchMemoryFiltersRequest,
    SearchMemoryRequest, SearchMemoryResponse, SearchMemoryResultResponse, UserMemoryExportResponse,
};
use crate::dto::memories::MemoryTypeResponse;
use crate::response::{ErrorBody, ErrorDetail};
use crate::routes::health::{HealthResponse, ReadyResponse};

/// OpenAPI document for the memcore HTTP API.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "memcore API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Production-grade long-term memory engine for AI agents. \
            Protected routes require `Authorization: Bearer <api_key>` and `X-Organization-ID`. \
            `MEMCORE_AUTH_MODE=dev` accepts `MEMCORE_DEV_API_KEY` for local development without scope checks."
    ),
    paths(
        paths::health,
        paths::ready,
        paths::metrics,
        paths::add_memory,
        paths::search_memory,
        paths::build_context,
        paths::list_user_memories,
        paths::delete_user_memory,
        paths::export_user_data,
        paths::import_user_data,
        paths::apply_retention,
        paths::forget_user,
        paths::list_user_memory_events,
        paths::create_api_key,
        paths::list_api_keys,
        paths::revoke_api_key,
        paths::get_org_summary,
        paths::list_org_users,
        paths::search_org_memory_events,
    ),
    components(schemas(
        ErrorBody,
        ErrorDetail,
        HealthResponse,
        ReadyResponse,
        AddMemoryRequest,
        MemoryMessageRequest,
        AddMemoryResponse,
        MemoryOperationSummaryResponse,
        MemoryItemResponse,
        MemoryTypeResponse,
        SearchMemoryRequest,
        SearchMemoryFiltersRequest,
        SearchMemoryResponse,
        SearchMemoryResultResponse,
        ListMemoriesResponse,
        ListMemoryItemResponse,
        DeleteMemoryResponse,
        ForgetUserResponse,
        ExportUserResponse,
        UserMemoryExportResponse,
        ExportFactItemResponse,
        ExportMemorySourceResponse,
        ImportUserDataRequest,
        ImportUserDataResponse,
        ImportUserDataSummaryResponse,
        ImportValidationIssueResponse,
        ImportValidationSummaryResponse,
        ApplyRetentionRequest,
        ApplyRetentionResponse,
        ApplyRetentionSummaryResponse,
        BuildContextRequest,
        BuildContextResponse,
        ContextBudgetResponse,
        ContextCompressionResponse,
        ContextCacheResponse,
        ContextMemoryResponse,
        ListMemoryEventsResponse,
        MemoryEventItemResponse,
        MemoryEventOperationResponse,
        CreateApiKeyRequest,
        CreateApiKeyResponse,
        ApiKeyItemResponse,
        ApiKeyScopeResponse,
        ListApiKeysResponse,
        RevokeApiKeyResponse,
        OrgSummaryResponse,
        OrgSummaryBodyResponse,
        ListOrgUsersResponse,
        OrgUserSummaryResponse,
        SearchOrgMemoryEventsResponse,
        AdminOrgMemoryEventItemResponse,
    )),
    tags(
        (name = "Health", description = "Liveness and readiness probes"),
        (name = "Metrics", description = "Basic in-process Prometheus metrics (local-dev oriented)"),
        (name = "Memories", description = "Memory lifecycle and search"),
        (name = "Context", description = "Context assembly for LLM prompts"),
        (name = "Users", description = "User-scoped data management"),
        (name = "Memory Events", description = "User-scoped memory audit events"),
        (name = "API Keys", description = "Organization API key management"),
        (name = "Admin", description = "Organization-level admin read endpoints"),
    ),
    modifiers(&SecurityAddon),
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let Some(components) = openapi.components.as_mut() else {
            return;
        };

        components.add_security_scheme(
            "BearerAuth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("API key")
                    .description(Some(
                        "Bearer API key. In dev mode use MEMCORE_DEV_API_KEY; in database mode use a stored key hash lookup.",
                    ))
                    .build(),
            ),
        );
    }
}
