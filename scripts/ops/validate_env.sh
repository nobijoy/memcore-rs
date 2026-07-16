#!/usr/bin/env bash
# Heuristic env-file validation for memcore (does not print secret values).
#
# Usage:
#   ./scripts/ops/validate_env.sh .env.production
#   ./scripts/ops/validate_env.sh .env.staging staging
#   ./scripts/ops/validate_env.sh .env.local local
#
# Modes: local | staging | production (optional second arg; inferred from MEMCORE_ENV / filename)
# Exit codes: 0 = ok (warnings allowed), 1 = usage/file error, 2 = hard failures

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <env-file> [local|staging|production]" >&2
  exit 1
fi

ENV_FILE="$1"
MODE_ARG="${2:-}"
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

is_example_file=0
case "$ENV_FILE" in
  *.example|*.example.*) is_example_file=1 ;;
esac

memcore_env="$(get_var MEMCORE_ENV)"
auth_mode="$(get_var MEMCORE_AUTH_MODE)"
fact_backend="$(get_var MEMCORE_FACT_BACKEND)"
event_backend="$(get_var MEMCORE_EVENT_BACKEND)"
vector_backend="$(get_var MEMCORE_VECTOR_BACKEND)"
migration_mode="$(get_var MEMCORE_DATABASE_MIGRATION_MODE)"
migrations_enabled="$(get_var MEMCORE_DATABASE_MIGRATIONS_ENABLED)"
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
metrics_enabled="$(get_var MEMCORE_METRICS_ENABLED)"
metrics_require_auth="$(get_var MEMCORE_METRICS_REQUIRE_AUTH)"
metrics_path="$(get_var MEMCORE_METRICS_PATH)"

env_normalized="$(printf '%s' "$memcore_env" | tr '[:upper:]' '[:lower:]')"
mode="$(printf '%s' "$MODE_ARG" | tr '[:upper:]' '[:lower:]')"
if [[ -z "$mode" ]]; then
  case "$env_normalized" in
    staging|stage) mode=staging ;;
    production|prod) mode=production ;;
    *)
      case "$ENV_FILE" in
        *.staging*|*.staging) mode=staging ;;
        *.production*|*.prod*) mode=production ;;
        *.local*) mode=local ;;
        *) mode=local ;;
      esac
      ;;
  esac
fi

case "$mode" in
  local|staging|production) ;;
  *)
    echo "error: unknown mode '$MODE_ARG' (expected local|staging|production)" >&2
    exit 1
    ;;
esac

is_staging=0
is_production=0
case "$mode" in
  staging) is_staging=1 ;;
  production) is_production=1 ;;
esac

# Placeholder scan for known sensitive keys (warn on examples; fail for live staging/prod files).
for key in MEMCORE_POSTGRES_URL MEMCORE_DATABASE_URL MEMCORE_DEV_API_KEY MEMCORE_API_KEY_PEPPER \
  MEMCORE_REDIS_URL OPENAI_API_KEY POSTGRES_PASSWORD; do
  val="$(get_var "$key")"
  if [[ -n "$val" ]] && has_placeholder "$val"; then
    if [[ "$is_example_file" -eq 1 ]]; then
      warn "$key still contains a placeholder token (expected in *.example files)"
    elif [[ "$is_staging" -eq 1 || "$is_production" -eq 1 ]]; then
      fail "$key still contains a placeholder token — replace before staging/production use"
    else
      warn "$key still contains a placeholder token"
    fi
  fi
done

if [[ -z "$vector_backend" ]]; then
  warn "MEMCORE_VECTOR_BACKEND is unset (code may default to LanceDB); set mock/qdrant/lancedb explicitly"
fi

if [[ "${restore_enabled}" == "true" ]]; then
  if [[ "$is_staging" -eq 1 || "$is_production" -eq 1 ]]; then
    fail "MEMCORE_RESTORE_ENABLED=true is unsafe for shared staging/production hosts"
  else
    warn "MEMCORE_RESTORE_ENABLED=true — destructive; keep false unless intentionally testing restore tooling"
  fi
fi

if [[ "${cors_enabled}" == "true" && "${cors_creds}" == "true" ]]; then
  if [[ "$cors_origins" == "*" || "$cors_origins" == *",*"* || "$cors_origins" == *"*,"* ]]; then
    fail "CORS credentials cannot be combined with wildcard origin *"
  fi
fi

if [[ "${metrics_enabled}" == "true" && "${metrics_require_auth}" == "false" ]]; then
  if [[ "$is_staging" -eq 1 || "$is_production" -eq 1 ]]; then
    fail "MEMCORE_METRICS_ENABLED=true with MEMCORE_METRICS_REQUIRE_AUTH=false — require auth or private-network-only exception documented elsewhere"
  else
    warn "metrics enabled without auth — use only on private loopback scrapes"
  fi
fi

if [[ "$is_staging" -eq 1 || "$is_production" -eq 1 ]]; then
  if [[ "${migrations_enabled}" == "false" ]]; then
    fail "MEMCORE_DATABASE_MIGRATIONS_ENABLED=false is not allowed for ${mode} validation"
  fi
  if [[ "${migration_mode:-auto}" == "disabled" ]]; then
    fail "MEMCORE_DATABASE_MIGRATION_MODE=disabled is not allowed for ${mode} validation"
  fi

  if [[ "${fact_backend}" == "sqlite" || "${event_backend}" == "sqlite" ]]; then
    fail "sqlite backends are not allowed for controlled ${mode} shape (use postgres)"
  fi

  if [[ "${fact_backend}" == "postgres" || "${event_backend}" == "postgres" ]]; then
    if [[ -z "$postgres_url" ]]; then
      fail "MEMCORE_POSTGRES_URL required when using postgres backends"
    fi
  else
    if [[ "$is_staging" -eq 1 ]]; then
      warn "controlled staging normally uses MEMCORE_FACT_BACKEND=postgres"
    fi
  fi

  if [[ "${vector_backend}" == "qdrant" ]]; then
    if [[ -z "$(get_var MEMCORE_QDRANT_URL)" ]]; then
      fail "MEMCORE_QDRANT_URL required when MEMCORE_VECTOR_BACKEND=qdrant"
    fi
  elif [[ "$is_staging" -eq 1 && "${vector_backend}" != "qdrant" ]]; then
    warn "controlled staging example uses qdrant; current MEMCORE_VECTOR_BACKEND=${vector_backend:-unset}"
  fi

  if [[ "${auth_mode:-dev}" == "database" && -z "$pepper" ]]; then
    fail "MEMCORE_API_KEY_PEPPER required when MEMCORE_AUTH_MODE=database"
  fi

  if [[ "${auth_mode:-dev}" == "dev" ]]; then
    if [[ "$is_production" -eq 1 ]]; then
      warn "MEMCORE_AUTH_MODE=dev in production-shaped env — prefer database auth"
    else
      warn "MEMCORE_AUTH_MODE=dev in staging — prefer database auth for shared staging"
    fi
  fi

  if [[ -n "$metrics_path" && "$metrics_path" != /* ]]; then
    fail "MEMCORE_METRICS_PATH must start with /"
  fi
fi

if [[ "${backup_enabled}" == "true" ]]; then
  dir="${backup_dir:-./backups}"
  if [[ -z "$backup_dir" ]]; then
    warn "MEMCORE_BACKUP_ENABLED=true but MEMCORE_BACKUP_DIR is empty"
  elif [[ ! -d "$dir" && "$is_example_file" -eq 0 ]]; then
    warn "MEMCORE_BACKUP_ENABLED=true but backup directory does not exist yet: (path omitted)"
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
echo "validate_env: checked $ENV_FILE mode=$mode (failures=$failures warnings=$warnings)"
if [[ "$is_example_file" -eq 1 ]]; then
  echo "note: example env files are expected to warn on CHANGE_ME placeholders"
fi
if [[ "$failures" -gt 0 ]]; then
  exit 2
fi
exit 0
