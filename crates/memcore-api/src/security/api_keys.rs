use memcore_common::hash_api_key;
use memcore_core::ApiKeyScope;
use uuid::Uuid;

pub use memcore_common::hash_api_key as hash_raw_api_key;

/// Authenticated API key context attached after successful database auth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub org_id: String,
    pub api_key_id: Uuid,
    pub scopes: Vec<ApiKeyScope>,
}

impl AuthContext {
    pub fn has_scope(&self, scope: ApiKeyScope) -> bool {
        self.scopes.iter().any(|value| *value == scope)
    }
}

pub fn hash_api_key_with_pepper(pepper: &str, raw_key: &str) -> String {
    hash_api_key(pepper, raw_key)
}
