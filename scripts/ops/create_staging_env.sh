#!/usr/bin/env bash
# Generate a local gitignored .env.staging from already-exported environment variables.
# Never prints secret values. Does not invent secrets.
#
# Usage:
#   export MEMCORE_POSTGRES_URL='postgres://...'
#   export POSTGRES_PASSWORD='...'
#   export MEMCORE_API_KEY_PEPPER='...'   # or MEMCORE_DEV_API_KEY for short-lived dev auth
#   # optional aliases: MEMCORE_DATABASE_URL -> MEMCORE_POSTGRES_URL
#   ./scripts/ops/create_staging_env.sh
#
# Overwrite existing file only with:
#   MEMCORE_OVERWRITE_STAGING_ENV=true ./scripts/ops/create_staging_env.sh
#
# See docs/runbooks/STAGING_SECRETS_INVENTORY.md

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

OUT=".env.staging"
EXAMPLE=".env.staging.example"

redacted_set() {
  local name="$1"
  local val="${!name-}"
  if [[ -n "$val" ]]; then
    echo "  $name=set"
  else
    echo "  $name=missing"
  fi
}

is_set() {
  local name="$1"
  [[ -n "${!name-}" ]]
}

fail() {
  echo "error: $*" >&2
  exit 1
}

if [[ ! -f "$EXAMPLE" ]]; then
  fail "missing $EXAMPLE"
fi

if [[ -f "$OUT" && "${MEMCORE_OVERWRITE_STAGING_ENV:-}" != "true" ]]; then
  fail "$OUT already exists; set MEMCORE_OVERWRITE_STAGING_ENV=true to overwrite"
fi

# Alias support (docs / access-request names)
if ! is_set MEMCORE_POSTGRES_URL && is_set MEMCORE_DATABASE_URL; then
  MEMCORE_POSTGRES_URL="$MEMCORE_DATABASE_URL"
  export MEMCORE_POSTGRES_URL
fi
if ! is_set MEMCORE_FACT_BACKEND && is_set MEMCORE_DATABASE_BACKEND; then
  MEMCORE_FACT_BACKEND="$MEMCORE_DATABASE_BACKEND"
  export MEMCORE_FACT_BACKEND
fi
if ! is_set OPENAI_API_KEY && is_set MEMCORE_OPENAI_API_KEY; then
  OPENAI_API_KEY="$MEMCORE_OPENAI_API_KEY"
  export OPENAI_API_KEY
fi

# Defaults for non-sensitive controlled staging shape
: "${MEMCORE_ENV:=production}"
: "${MEMCORE_HOST:=0.0.0.0}"
: "${MEMCORE_PORT:=8080}"
: "${MEMCORE_STORAGE_MODE:=production}"
: "${MEMCORE_FACT_BACKEND:=postgres}"
: "${MEMCORE_EVENT_BACKEND:=postgres}"
: "${MEMCORE_VECTOR_BACKEND:=qdrant}"
: "${MEMCORE_QDRANT_URL:=http://qdrant:6334}"
: "${MEMCORE_QDRANT_COLLECTION:=memcore_staging}"
: "${MEMCORE_DATABASE_MIGRATIONS_ENABLED:=true}"
: "${MEMCORE_DATABASE_MIGRATION_MODE:=auto}"
: "${MEMCORE_DATABASE_REQUIRE_CLEAN_MIGRATIONS:=true}"
: "${MEMCORE_AUTH_ENABLED:=true}"
: "${MEMCORE_AUTH_MODE:=database}"
: "${MEMCORE_LLM_PROVIDER:=mock}"
: "${MEMCORE_LLM_MODEL:=mock-llm}"
: "${MEMCORE_EMBEDDING_PROVIDER:=mock}"
: "${MEMCORE_EMBEDDING_MODEL:=mock-embedding}"
: "${MEMCORE_CONTEXT_CACHE_BACKEND:=disabled}"
: "${MEMCORE_METRICS_ENABLED:=true}"
: "${MEMCORE_METRICS_PATH:=/metrics}"
: "${MEMCORE_METRICS_REQUIRE_AUTH:=true}"
: "${MEMCORE_CORS_ENABLED:=false}"
: "${MEMCORE_CORS_ALLOW_CREDENTIALS:=false}"
: "${MEMCORE_RESTORE_ENABLED:=false}"
: "${MEMCORE_BACKUP_ENABLED:=false}"
: "${MEMCORE_BACKUP_DIR:=/var/lib/memcore/backups}"
: "${MEMCORE_BACKGROUND_JOBS_ENABLED:=true}"
: "${MEMCORE_BACKGROUND_JOB_ORG_IDS:=org_staging}"
: "${MEMCORE_BACKGROUND_JOB_LOCK_ENABLED:=true}"
: "${MEMCORE_BACKGROUND_JOB_LOCK_BACKEND:=database}"
: "${MEMCORE_BACKGROUND_JOB_HISTORY_ENABLED:=true}"
: "${MEMCORE_LOG_FORMAT:=json}"
: "${MEMCORE_LOG_LEVEL:=info}"
: "${MEMCORE_SECURITY_HEADERS_ENABLED:=true}"
: "${MEMCORE_RATE_LIMIT_ENABLED:=true}"
: "${POSTGRES_DB:=memcore_staging}"
: "${POSTGRES_USER:=memcore}"

export MEMCORE_ENV MEMCORE_HOST MEMCORE_PORT MEMCORE_STORAGE_MODE
export MEMCORE_FACT_BACKEND MEMCORE_EVENT_BACKEND
export MEMCORE_VECTOR_BACKEND MEMCORE_QDRANT_URL MEMCORE_QDRANT_COLLECTION
export MEMCORE_DATABASE_MIGRATIONS_ENABLED MEMCORE_DATABASE_MIGRATION_MODE
export MEMCORE_DATABASE_REQUIRE_CLEAN_MIGRATIONS
export MEMCORE_AUTH_ENABLED MEMCORE_AUTH_MODE
export MEMCORE_LLM_PROVIDER MEMCORE_LLM_MODEL MEMCORE_EMBEDDING_PROVIDER MEMCORE_EMBEDDING_MODEL
export MEMCORE_CONTEXT_CACHE_BACKEND
export MEMCORE_METRICS_ENABLED MEMCORE_METRICS_PATH MEMCORE_METRICS_REQUIRE_AUTH
export MEMCORE_CORS_ENABLED MEMCORE_CORS_ALLOW_CREDENTIALS
export MEMCORE_RESTORE_ENABLED MEMCORE_BACKUP_ENABLED MEMCORE_BACKUP_DIR
export MEMCORE_BACKGROUND_JOBS_ENABLED MEMCORE_BACKGROUND_JOB_ORG_IDS
export MEMCORE_BACKGROUND_JOB_LOCK_ENABLED MEMCORE_BACKGROUND_JOB_LOCK_BACKEND
export MEMCORE_BACKGROUND_JOB_HISTORY_ENABLED
export MEMCORE_LOG_FORMAT MEMCORE_LOG_LEVEL MEMCORE_SECURITY_HEADERS_ENABLED MEMCORE_RATE_LIMIT_ENABLED
export POSTGRES_DB POSTGRES_USER

missing=0
require() {
  local name="$1"
  if ! is_set "$name"; then
    echo "error: required variable not set: $name" >&2
    missing=1
  fi
}

require MEMCORE_POSTGRES_URL
require POSTGRES_PASSWORD

if [[ "${MEMCORE_AUTH_MODE}" == "database" ]]; then
  require MEMCORE_API_KEY_PEPPER
elif [[ "${MEMCORE_AUTH_MODE}" == "dev" ]]; then
  require MEMCORE_DEV_API_KEY
fi

if [[ "${MEMCORE_CONTEXT_CACHE_BACKEND}" == "redis" ]]; then
  require MEMCORE_REDIS_URL
fi

if [[ "${MEMCORE_LLM_PROVIDER}" == "openai" || "${MEMCORE_EMBEDDING_PROVIDER}" == "openai" ]]; then
  require OPENAI_API_KEY
fi

if [[ "$missing" -ne 0 ]]; then
  echo "error: export operator-provided values first (see STAGING_SECRETS_INVENTORY.md)" >&2
  exit 1
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

{
  echo "# Generated by scripts/ops/create_staging_env.sh — DO NOT COMMIT"
  echo "# Local gitignored staging env. Secrets omitted from this comment."
  echo ""
  echo "MEMCORE_ENV=${MEMCORE_ENV}"
  echo "MEMCORE_HOST=${MEMCORE_HOST}"
  echo "MEMCORE_PORT=${MEMCORE_PORT}"
  echo ""
  echo "MEMCORE_AUTH_ENABLED=${MEMCORE_AUTH_ENABLED}"
  echo "MEMCORE_AUTH_MODE=${MEMCORE_AUTH_MODE}"
  if is_set MEMCORE_API_KEY_PEPPER; then
    echo "MEMCORE_API_KEY_PEPPER=${MEMCORE_API_KEY_PEPPER}"
  fi
  if is_set MEMCORE_DEV_API_KEY; then
    echo "MEMCORE_DEV_API_KEY=${MEMCORE_DEV_API_KEY}"
  fi
  echo ""
  echo "MEMCORE_STORAGE_MODE=${MEMCORE_STORAGE_MODE}"
  echo "MEMCORE_FACT_BACKEND=${MEMCORE_FACT_BACKEND}"
  echo "MEMCORE_EVENT_BACKEND=${MEMCORE_EVENT_BACKEND}"
  echo "MEMCORE_POSTGRES_URL=${MEMCORE_POSTGRES_URL}"
  echo "MEMCORE_DATABASE_MIGRATIONS_ENABLED=${MEMCORE_DATABASE_MIGRATIONS_ENABLED}"
  echo "MEMCORE_DATABASE_MIGRATION_MODE=${MEMCORE_DATABASE_MIGRATION_MODE}"
  echo "MEMCORE_DATABASE_REQUIRE_CLEAN_MIGRATIONS=${MEMCORE_DATABASE_REQUIRE_CLEAN_MIGRATIONS}"
  echo "POSTGRES_DB=${POSTGRES_DB}"
  echo "POSTGRES_USER=${POSTGRES_USER}"
  echo "POSTGRES_PASSWORD=${POSTGRES_PASSWORD}"
  echo ""
  echo "MEMCORE_VECTOR_BACKEND=${MEMCORE_VECTOR_BACKEND}"
  echo "MEMCORE_QDRANT_URL=${MEMCORE_QDRANT_URL}"
  echo "MEMCORE_QDRANT_COLLECTION=${MEMCORE_QDRANT_COLLECTION}"
  echo ""
  echo "MEMCORE_LLM_PROVIDER=${MEMCORE_LLM_PROVIDER}"
  echo "MEMCORE_LLM_MODEL=${MEMCORE_LLM_MODEL}"
  echo "MEMCORE_EMBEDDING_PROVIDER=${MEMCORE_EMBEDDING_PROVIDER}"
  echo "MEMCORE_EMBEDDING_MODEL=${MEMCORE_EMBEDDING_MODEL}"
  if is_set OPENAI_API_KEY; then
    echo "OPENAI_API_KEY=${OPENAI_API_KEY}"
  fi
  if is_set OPENAI_BASE_URL; then
    echo "OPENAI_BASE_URL=${OPENAI_BASE_URL}"
  fi
  echo ""
  echo "MEMCORE_CONTEXT_CACHE_BACKEND=${MEMCORE_CONTEXT_CACHE_BACKEND}"
  if is_set MEMCORE_REDIS_URL; then
    echo "MEMCORE_REDIS_URL=${MEMCORE_REDIS_URL}"
  fi
  echo ""
  echo "MEMCORE_BACKGROUND_JOBS_ENABLED=${MEMCORE_BACKGROUND_JOBS_ENABLED}"
  echo "MEMCORE_BACKGROUND_JOB_ORG_IDS=${MEMCORE_BACKGROUND_JOB_ORG_IDS}"
  echo "MEMCORE_BACKGROUND_JOB_LOCK_ENABLED=${MEMCORE_BACKGROUND_JOB_LOCK_ENABLED}"
  echo "MEMCORE_BACKGROUND_JOB_LOCK_BACKEND=${MEMCORE_BACKGROUND_JOB_LOCK_BACKEND}"
  echo "MEMCORE_BACKGROUND_JOB_HISTORY_ENABLED=${MEMCORE_BACKGROUND_JOB_HISTORY_ENABLED}"
  echo ""
  echo "MEMCORE_RATE_LIMIT_ENABLED=${MEMCORE_RATE_LIMIT_ENABLED}"
  echo "MEMCORE_SECURITY_HEADERS_ENABLED=${MEMCORE_SECURITY_HEADERS_ENABLED}"
  echo "MEMCORE_CORS_ENABLED=${MEMCORE_CORS_ENABLED}"
  echo "MEMCORE_CORS_ALLOW_CREDENTIALS=${MEMCORE_CORS_ALLOW_CREDENTIALS}"
  if is_set MEMCORE_CORS_ALLOWED_ORIGINS; then
    echo "MEMCORE_CORS_ALLOWED_ORIGINS=${MEMCORE_CORS_ALLOWED_ORIGINS}"
  fi
  echo "MEMCORE_RESTORE_ENABLED=${MEMCORE_RESTORE_ENABLED}"
  echo "MEMCORE_BACKUP_ENABLED=${MEMCORE_BACKUP_ENABLED}"
  echo "MEMCORE_BACKUP_DIR=${MEMCORE_BACKUP_DIR}"
  echo ""
  echo "MEMCORE_LOG_FORMAT=${MEMCORE_LOG_FORMAT}"
  echo "MEMCORE_LOG_LEVEL=${MEMCORE_LOG_LEVEL}"
  echo "MEMCORE_METRICS_ENABLED=${MEMCORE_METRICS_ENABLED}"
  echo "MEMCORE_METRICS_PATH=${MEMCORE_METRICS_PATH}"
  echo "MEMCORE_METRICS_REQUIRE_AUTH=${MEMCORE_METRICS_REQUIRE_AUTH}"
  # Optional smoke helpers in file (prefer shell exports for live checks)
  if is_set MEMCORE_SMOKE_TEST_API_KEY; then
    echo "MEMCORE_SMOKE_TEST_API_KEY=${MEMCORE_SMOKE_TEST_API_KEY}"
  fi
  if is_set MEMCORE_SMOKE_TEST_ORG_ID; then
    echo "MEMCORE_SMOKE_TEST_ORG_ID=${MEMCORE_SMOKE_TEST_ORG_ID}"
  fi
  if is_set MEMCORE_SMOKE_TEST_USER_ID; then
    echo "MEMCORE_SMOKE_TEST_USER_ID=${MEMCORE_SMOKE_TEST_USER_ID}"
  fi
  if is_set MEMCORE_METRICS_API_KEY; then
    echo "MEMCORE_METRICS_API_KEY=${MEMCORE_METRICS_API_KEY}"
  fi
  if is_set MEMCORE_STAGING_BASE_URL; then
    echo "MEMCORE_STAGING_BASE_URL=${MEMCORE_STAGING_BASE_URL}"
  fi
} >"$tmp"

if grep -q 'CHANGE_ME' "$tmp"; then
  fail "generated file still contains CHANGE_ME — refuse to write"
fi

mv "$tmp" "$OUT"
trap - EXIT
chmod 600 "$OUT" 2>/dev/null || true

if ! git check-ignore -q "$OUT" 2>/dev/null; then
  echo "error: $OUT is not ignored by Git — fix .gitignore before continuing" >&2
  exit 1
fi

echo "create_staging_env: wrote $OUT (permissions tightened if supported)"
echo "create_staging_env: redacted summary (set/missing only):"
redacted_set MEMCORE_ENV
redacted_set MEMCORE_FACT_BACKEND
redacted_set MEMCORE_POSTGRES_URL
redacted_set POSTGRES_PASSWORD
redacted_set MEMCORE_AUTH_MODE
redacted_set MEMCORE_API_KEY_PEPPER
redacted_set MEMCORE_DEV_API_KEY
redacted_set MEMCORE_VECTOR_BACKEND
redacted_set MEMCORE_QDRANT_URL
redacted_set MEMCORE_LLM_PROVIDER
redacted_set OPENAI_API_KEY
redacted_set MEMCORE_REDIS_URL
redacted_set MEMCORE_METRICS_ENABLED
redacted_set MEMCORE_SMOKE_TEST_API_KEY
redacted_set MEMCORE_METRICS_API_KEY
redacted_set MEMCORE_STAGING_BASE_URL

echo "create_staging_env: running validate_env.sh …"
./scripts/ops/validate_env.sh "$OUT" staging
echo "create_staging_env: ok — next: validate_env.sh $OUT staging --live (with smoke/metrics exports)"
