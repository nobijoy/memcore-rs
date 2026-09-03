#!/usr/bin/env bash
# Local Docker staging Postgres backup / isolated restore dry-run.
# Does NOT overwrite the active staging database.
# Does NOT print secrets. Does NOT run against production.
#
# Usage (repo root, with .env.staging present):
#   ./scripts/ops/local_staging_backup_dry_run.sh
#
# Optional:
#   MEMCORE_STAGING_BASE_URL=http://localhost:8080
#   COMPOSE_FILE=docker/docker-compose.staging.example.yml
#   ENV_FILE=.env.staging

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

COMPOSE_FILE="${COMPOSE_FILE:-docker/docker-compose.staging.example.yml}"
ENV_FILE="${ENV_FILE:-.env.staging}"
BASE_URL="${MEMCORE_STAGING_BASE_URL:-http://localhost:8080}"
OUT_DIR="reports/staging-backups"
STAMP="$(date -u +%Y%m%d_%H%M%S)"
DUMP_HOST="${OUT_DIR}/memcore_staging_backup_${STAMP}.dump"
DUMP_CTR="/tmp/memcore_staging_backup.dump"
DB_USER="${POSTGRES_USER:-memcore}"
DB_ACTIVE="${POSTGRES_DB:-memcore_staging}"
DB_RESTORE="memcore_staging_restore_check"

fail() { echo "error: $*" >&2; exit 1; }

# Refuse production-looking targets.
case "$BASE_URL" in
  *localhost*|*127.0.0.1*) ;;
  *) fail "refusing non-local base URL (got a non-localhost value; local Docker only)" ;;
esac
case "$ENV_FILE" in
  *.production*|*.prod*) fail "refusing production env file name" ;;
esac
if [[ ! -f "$ENV_FILE" ]]; then
  fail "missing $ENV_FILE (local gitignored staging env required)"
fi
if grep -Eqi '^[[:space:]]*MEMCORE_ENV=.*(prod).*' "$ENV_FILE" 2>/dev/null; then
  # production enum is expected for staging shape; still require local compose file.
  :
fi
if [[ "$COMPOSE_FILE" != *staging* ]]; then
  fail "refusing compose file that does not look like staging ($COMPOSE_FILE)"
fi

command -v docker >/dev/null 2>&1 || fail "docker required"
command -v curl >/dev/null 2>&1 || fail "curl required"

compose() {
  docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" "$@"
}

mkdir -p "$OUT_DIR"

echo "local_staging_backup_dry_run: checking readiness at (host omitted if secret)…"
curl -fsS "${BASE_URL}/ready" >/dev/null || fail "/ready failed — start local staging first"
echo "ok: /ready"

echo "ok: creating Postgres custom dump in container (password not printed)"
compose exec -T postgres \
  pg_dump -U "$DB_USER" -d "$DB_ACTIVE" --format=custom --file="$DUMP_CTR"

echo "ok: copying dump to $DUMP_HOST"
compose cp "postgres:${DUMP_CTR}" "$DUMP_HOST"
if [[ ! -s "$DUMP_HOST" ]]; then
  fail "dump missing or empty: $DUMP_HOST"
fi
SIZE="$(wc -c < "$DUMP_HOST" | tr -d ' ')"
echo "ok: dump size_bytes=${SIZE}"

echo "ok: validating dump with pg_restore --list"
LIST_LINES="$(compose exec -T postgres pg_restore --list "$DUMP_CTR" | wc -l | tr -d ' ')"
echo "ok: pg_restore_list_lines=${LIST_LINES}"

ACTIVE_MIG="$(compose exec -T postgres psql -U "$DB_USER" -d "$DB_ACTIVE" -tAc 'SELECT COUNT(*) FROM schema_migrations;' | tr -d '[:space:]')"
echo "ok: active schema_migrations=${ACTIVE_MIG}"

echo "ok: creating isolated restore DB ${DB_RESTORE}"
compose exec -T postgres dropdb -U "$DB_USER" --if-exists "$DB_RESTORE" >/dev/null
compose exec -T postgres createdb -U "$DB_USER" "$DB_RESTORE"
compose exec -T postgres pg_restore -U "$DB_USER" -d "$DB_RESTORE" "$DUMP_CTR"

RESTORE_MIG="$(compose exec -T postgres psql -U "$DB_USER" -d "$DB_RESTORE" -tAc 'SELECT COUNT(*) FROM schema_migrations;' | tr -d '[:space:]')"
echo "ok: restore schema_migrations=${RESTORE_MIG}"
if [[ "$ACTIVE_MIG" != "$RESTORE_MIG" ]]; then
  fail "migration count mismatch active=${ACTIVE_MIG} restore=${RESTORE_MIG}"
fi

echo "ok: dropping isolated restore DB only"
compose exec -T postgres dropdb -U "$DB_USER" "$DB_RESTORE"

# Confirm active DB still responds
compose exec -T postgres psql -U "$DB_USER" -d "$DB_ACTIVE" -tAc 'SELECT 1;' >/dev/null
echo "ok: active DB still alive"

echo "local_staging_backup_dry_run: PASSED"
echo "  dump=${DUMP_HOST}"
echo "  size_bytes=${SIZE}"
echo "  migrations=${ACTIVE_MIG}"
echo "  note: dump is gitignored; do not commit; do not restore over active DB without approval"
exit 0
