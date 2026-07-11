#!/usr/bin/env bash
# Basic smoke test against a running memcore API.
#
# Usage:
#   ./scripts/ops/smoke_test.sh http://localhost:8080
#   MEMCORE_SMOKE_TEST_API_KEY=... MEMCORE_SMOKE_TEST_ORG_ID=org_demo \
#     ./scripts/ops/smoke_test.sh http://localhost:8080
#
# Never prints the API key. Requires curl. jq is optional.

set -euo pipefail

BASE_URL="${1:-}"
if [[ -z "$BASE_URL" ]]; then
  echo "usage: $0 <base-url>" >&2
  exit 1
fi

# Trim trailing slash
BASE_URL="${BASE_URL%/}"

if ! command -v curl >/dev/null 2>&1; then
  echo "error: curl is required" >&2
  exit 1
fi

API_KEY="${MEMCORE_SMOKE_TEST_API_KEY:-}"
ORG_ID="${MEMCORE_SMOKE_TEST_ORG_ID:-org_smoke}"

check_get() {
  local path="$1"
  local url="${BASE_URL}${path}"
  local code
  local body
  body="$(mktemp)"
  code="$(curl -sS -o "$body" -w '%{http_code}' "$url" || true)"
  if [[ "$code" != 2* ]]; then
    echo "error: GET $path returned HTTP $code" >&2
    head -c 500 "$body" >&2 || true
    echo >&2
    rm -f "$body"
    exit 1
  fi
  echo "ok: GET $path -> $code"
  if command -v jq >/dev/null 2>&1; then
    jq -e . >/dev/null 2>&1 < "$body" \
      || echo "warning: response was not JSON for $path" >&2
  fi
  rm -f "$body"
}

check_get /health
check_get /ready
check_get /api/v1/version

if [[ -n "$API_KEY" ]]; then
  url="${BASE_URL}/api/v1/admin/org/summary"
  body="$(mktemp)"
  code="$(curl -sS -o "$body" -w '%{http_code}' \
    -H "Authorization: Bearer ${API_KEY}" \
    -H "X-Organization-ID: ${ORG_ID}" \
    "$url" || true)"
  # Do not print Authorization header or key.
  if [[ "$code" != 2* ]]; then
    echo "error: authenticated GET /api/v1/admin/org/summary returned HTTP $code" >&2
    head -c 500 "$body" >&2 || true
    echo >&2
    rm -f "$body"
    exit 1
  fi
  echo "ok: authenticated GET /api/v1/admin/org/summary -> $code"
  rm -f "$body"
else
  echo "skip: authenticated check (set MEMCORE_SMOKE_TEST_API_KEY to enable)"
fi

echo "smoke_test: passed against $BASE_URL"
