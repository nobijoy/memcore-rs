#!/usr/bin/env bash
# Safe smoke test against a running memcore API.
#
# Unauthenticated (default — read-only operational checks):
#   ./scripts/ops/smoke_test.sh http://localhost:8080
#
# Authenticated write/read/delete (requires explicit flag):
#   MEMCORE_SMOKE_TEST_API_KEY=... \
#   MEMCORE_SMOKE_TEST_ORG_ID=org_smoke \
#   MEMCORE_SMOKE_TEST_USER_ID=smoke-test-user \
#   ./scripts/ops/smoke_test.sh http://localhost:8080 --authenticated
#
# Never prints the API key. Requires curl. jq is optional.
# Do not run destructive broad cleanup. Authenticated mode only touches the smoke-test user.

set -euo pipefail

BASE_URL=""
AUTHENTICATED=0

usage() {
  echo "usage: $0 <base-url> [--authenticated]" >&2
  echo "  Unauthenticated: GET /health /ready /api/v1/version" >&2
  echo "  --authenticated: also create/search/context/delete a smoke-test memory" >&2
  echo "  Requires MEMCORE_SMOKE_TEST_API_KEY (and preferably MEMCORE_SMOKE_TEST_ORG_ID)" >&2
  exit 1
}

for arg in "$@"; do
  case "$arg" in
    --authenticated) AUTHENTICATED=1 ;;
    -h|--help) usage ;;
    http://*|https://*) BASE_URL="$arg" ;;
    *)
      if [[ -z "$BASE_URL" ]]; then
        BASE_URL="$arg"
      else
        echo "error: unexpected argument: $arg" >&2
        usage
      fi
      ;;
  esac
done

if [[ -z "$BASE_URL" ]]; then
  usage
fi

# Trim trailing slash
BASE_URL="${BASE_URL%/}"

if ! command -v curl >/dev/null 2>&1; then
  echo "error: curl is required" >&2
  exit 1
fi

API_KEY="${MEMCORE_SMOKE_TEST_API_KEY:-}"
ORG_ID="${MEMCORE_SMOKE_TEST_ORG_ID:-org_smoke}"
USER_ID="${MEMCORE_SMOKE_TEST_USER_ID:-smoke-test-user}"

fail() {
  echo "error: $*" >&2
  exit 1
}

# Never echo API_KEY / Authorization values.
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

auth_curl() {
  # Args: method path [json_body]
  # Writes response body to stdout; prints status code on fd 3 via caller pattern.
  local method="$1"
  local path="$2"
  local json_body="${3:-}"
  local url="${BASE_URL}${path}"
  local body code
  body="$(mktemp)"

  if [[ -n "$json_body" ]]; then
    code="$(curl -sS -o "$body" -w '%{http_code}' \
      -X "$method" \
      -H "Authorization: Bearer ${API_KEY}" \
      -H "X-Organization-ID: ${ORG_ID}" \
      -H "Content-Type: application/json" \
      -d "$json_body" \
      "$url" || true)"
  else
    code="$(curl -sS -o "$body" -w '%{http_code}' \
      -X "$method" \
      -H "Authorization: Bearer ${API_KEY}" \
      -H "X-Organization-ID: ${ORG_ID}" \
      "$url" || true)"
  fi

  if [[ "$code" != 2* ]]; then
    echo "error: ${method} ${path} returned HTTP ${code}" >&2
    head -c 500 "$body" >&2 || true
    echo >&2
    rm -f "$body"
    exit 1
  fi

  echo "ok: ${method} ${path} -> ${code}"
  # shellcheck disable=SC2034
  AUTH_CURL_BODY="$body"
  AUTH_CURL_CODE="$code"
}

echo "smoke_test: base=$BASE_URL authenticated=$AUTHENTICATED"

check_get /health
check_get /ready
check_get /api/v1/version

if [[ "$AUTHENTICATED" -eq 1 ]]; then
  if [[ -z "$API_KEY" ]]; then
    fail "MEMCORE_SMOKE_TEST_API_KEY is required with --authenticated"
  fi

  echo "ok: using smoke-test user_id=${USER_ID} org_id=${ORG_ID} (API key not printed)"

  # Create a small synthetic memory for the dedicated smoke-test user only.
  CREATE_BODY=$(cat <<EOF
{"user_id":"${USER_ID}","messages":[{"role":"user","content":"Smoke test memory: user likes green tea."}],"metadata":{"source":"smoke_test"}}
EOF
)

  AUTH_CURL_BODY=""
  auth_curl POST /api/v1/memories "$CREATE_BODY"
  create_body_file="$AUTH_CURL_BODY"

  MEMORY_ID=""
  if command -v jq >/dev/null 2>&1; then
    MEMORY_ID="$(jq -r '.memories[0].id // empty' < "$create_body_file" 2>/dev/null || true)"
  fi
  rm -f "$create_body_file"

  SEARCH_BODY=$(cat <<EOF
{"user_id":"${USER_ID}","query":"green tea"}
EOF
)
  auth_curl POST /api/v1/memories/search "$SEARCH_BODY"
  rm -f "$AUTH_CURL_BODY"

  CONTEXT_BODY=$(cat <<EOF
{"user_id":"${USER_ID}","query":"green tea","max_memories":5}
EOF
)
  auth_curl POST /api/v1/context "$CONTEXT_BODY"
  rm -f "$AUTH_CURL_BODY"

  auth_curl GET "/api/v1/users/${USER_ID}/memories?limit=20"
  rm -f "$AUTH_CURL_BODY"

  if [[ -n "$MEMORY_ID" && "$MEMORY_ID" != "null" ]]; then
    auth_curl DELETE "/api/v1/users/${USER_ID}/memories/${MEMORY_ID}"
    rm -f "$AUTH_CURL_BODY"
    echo "ok: cleaned up smoke-test memory id=${MEMORY_ID}"
  else
    echo "warning: could not parse memory id for cleanup (jq missing or unexpected response); leaving smoke-test user data" >&2
  fi
elif [[ -n "$API_KEY" ]]; then
  # Backward-compatible optional read-only admin probe when key is set without --authenticated.
  url="${BASE_URL}/api/v1/admin/org/summary"
  body="$(mktemp)"
  code="$(curl -sS -o "$body" -w '%{http_code}' \
    -H "Authorization: Bearer ${API_KEY}" \
    -H "X-Organization-ID: ${ORG_ID}" \
    "$url" || true)"
  if [[ "$code" != 2* ]]; then
    echo "error: authenticated GET /api/v1/admin/org/summary returned HTTP $code" >&2
    head -c 500 "$body" >&2 || true
    echo >&2
    rm -f "$body"
    exit 1
  fi
  echo "ok: authenticated GET /api/v1/admin/org/summary -> $code"
  echo "note: pass --authenticated for create/search/context/delete smoke flow"
  rm -f "$body"
else
  echo "skip: authenticated checks (set MEMCORE_SMOKE_TEST_API_KEY and optionally --authenticated)"
fi

echo "smoke_test: passed against $BASE_URL"
