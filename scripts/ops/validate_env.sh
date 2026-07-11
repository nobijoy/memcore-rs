#!/usr/bin/env bash
# Heuristic env-file validation for memcore (does not print secret values).
#
# Usage:
#   ./scripts/ops/validate_env.sh .env.production
#   ./scripts/ops/validate_env.sh .env.local
#
# Exit codes: 0 = ok (warnings allowed), 1 = usage/file error, 2 = hard failures

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <env-file>" >&2
  exit 1
fi

ENV_FILE="$1"
if [[ ! -f "$ENV_FILE" ]]; then
  echo "error: file not found: $ENV_FILE" >&2
  exit 1
fi

warnings=0
failures=0

warn() {
  echo "warning: $*" >&2
  warnings=$((warnings + 1))
}

fail() {
  echo "error: $*" >&2
  failures=$((failures + 1))
}

# Read KEY=VALUE lines (no export, no multiline). Does not echo values.
get_var() {
  local key="$1"
  local line
  line="$(grep -E "^[[:space:]]*${key}=" "$ENV_FILE" | tail -n1 || true)"
  if [[ -z "$line" ]]; then
    echo ""
    return
  fi
  echo "${line#*=}" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//;s/^"//;s/"$//;s/^'"'"'//;s/'"'"'$//'
}

has_placeholder() {
  local value="$1"
  [[ "$value" == *CHANGE_ME* ]] \
    || [[ "$value" == *your-openai-api-key* ]] \
    || [[ "$value" == *your_openai_api_key* ]] \
    || [[ "$value" == *change_this* ]]
}

memcore_env="$(get_var MEMCORE_ENV)"
auth_mode="$(get_var MEMCORE_AUTH_MODE)"
fact_backend="$(get_var MEMCORE_FACT_BACKEND)"
event_backend="$(get_var MEMCORE_EVENT_BACKEND)"
vector_backend="$(get_var MEMCORE_VECTOR_BACKEND)"
migration_mode="$(get_var MEMCORE_DATABASE_MIGRATION_MODE)"
cors_enabled="$(get_var MEMCORE_CORS_ENABLED)"
cors_origins="$(get_var MEMCORE_CORS_ALLOWED_ORIGINS)"
cors_creds="$(get_var MEMCORE_CORS_ALLOW_CREDENTIALS)"
backup_enabled="$(get_var MEMCORE_BACKUP_ENABLED)"
backup_dir="$(get_var MEMCORE_BACKUP_DIR)"
restore_enabled="$(get_var MEMCORE_RESTORE_ENABLED)"
postgres_url="$(get_var MEMCORE_POSTGRES_URL)"
dev_key="$(get_var MEMCORE_DEV_API_KEY)"
pepper="$(get_var MEMCORE_API_KEY_PEPPER)"
openai_key="$(get_var OPENAI_API_KEY)"
redis_url="$(get_var MEMCORE_REDIS_URL)"
cache_backend="$(get_var MEMCORE_CONTEXT_CACHE_BACKEND)"
llm_provider="$(get_var MEMCORE_LLM_PROVIDER)"
embed_provider="$(get_var MEMCORE_EMBEDDING_PROVIDER)"
postgres_password="$(get_var POSTGRES_PASSWORD)"

is_production=0
env_normalized="$(printf '%s' "$memcore_env" | tr '[:upper:]' '[:lower:]')"
case "$env_normalized" in
  production|prod) is_production=1 ;;
esac

# Placeholder scan for known sensitive keys (warn only — report key names, never values).
for key in MEMCORE_POSTGRES_URL MEMCORE_DATABASE_URL MEMCORE_DEV_API_KEY MEMCORE_API_KEY_PEPPER \
  MEMCORE_REDIS_URL OPENAI_API_KEY POSTGRES_PASSWORD; do
  val="$(get_var "$key")"
  if [[ -n "$val" ]] && has_placeholder "$val"; then
    warn "$key still contains a placeholder token"
  fi
done

if [[ -z "$vector_backend" ]]; then
  warn "MEMCORE_VECTOR_BACKEND is unset (code may default to LanceDB); set mock/qdrant/lancedb explicitly"
fi

if [[ "$is_production" -eq 1 ]]; then
  if [[ "${migration_mode:-auto}" == "disabled" ]]; then
    warn "MEMCORE_DATABASE_MIGRATION_MODE=disabled in production — ensure external migrations are managed"
  fi

  if [[ "${fact_backend}" == "postgres" || "${event_backend}" == "postgres" ]]; then
    if [[ -z "$postgres_url" ]]; then
      fail "MEMCORE_POSTGRES_URL required when using postgres backends"
    fi
  fi

  if [[ "${vector_backend}" == "qdrant" ]]; then
    if [[ -z "$(get_var MEMCORE_QDRANT_URL)" ]]; then
      fail "MEMCORE_QDRANT_URL required when MEMCORE_VECTOR_BACKEND=qdrant"
    fi
  fi

  if [[ "${auth_mode:-dev}" == "database" && -z "$pepper" ]]; then
    fail "MEMCORE_API_KEY_PEPPER required when MEMCORE_AUTH_MODE=database"
  fi

  if [[ "${auth_mode:-dev}" == "dev" ]]; then
    warn "MEMCORE_AUTH_MODE=dev in production-shaped env — prefer database auth"
  fi

  if [[ "${restore_enabled}" == "true" ]]; then
    fail "MEMCORE_RESTORE_ENABLED=true is unsafe for shared production hosts"
  fi
fi

if [[ "${cors_enabled}" == "true" && "${cors_creds}" == "true" ]]; then
  if [[ "$cors_origins" == "*" || "$cors_origins" == *",*"* || "$cors_origins" == *"*,"* ]]; then
    fail "CORS credentials cannot be combined with wildcard origin *"
  fi
fi

if [[ "${backup_enabled}" == "true" ]]; then
  dir="${backup_dir:-./backups}"
  if [[ ! -d "$dir" ]]; then
    warn "MEMCORE_BACKUP_ENABLED=true but backup directory does not exist yet: $dir"
  fi
fi

if [[ "${cache_backend}" == "redis" && -z "$redis_url" ]]; then
  fail "MEMCORE_REDIS_URL required when MEMCORE_CONTEXT_CACHE_BACKEND=redis"
fi

if [[ "${llm_provider}" == "openai" || "${embed_provider}" == "openai" ]]; then
  if [[ -z "$openai_key" ]]; then
    fail "OPENAI_API_KEY required when OpenAI providers are selected"
  fi
fi

# Avoid echoing any secret content.
echo "validate_env: checked $ENV_FILE (failures=$failures warnings=$warnings)"
if [[ "$failures" -gt 0 ]]; then
  exit 2
fi
exit 0
