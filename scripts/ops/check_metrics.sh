#!/usr/bin/env bash
# Validate Prometheus scrape against a running memcore API.
#
# Usage:
#   MEMCORE_METRICS_API_KEY=... ./scripts/ops/check_metrics.sh https://staging.example.com
#   MEMCORE_METRICS_STRICT=true MEMCORE_METRICS_API_KEY=... ./scripts/ops/check_metrics.sh https://staging.example.com
#
# Never prints API keys or bearer tokens.
# Exit 0: metrics OK (or gracefully unavailable when not strict)
# Exit 1: usage / curl missing
# Exit 2: metrics check failed (strict or unexpected content)

set -euo pipefail

BASE_URL="${1:-}"
if [[ -z "$BASE_URL" || "$BASE_URL" == "-h" || "$BASE_URL" == "--help" ]]; then
  echo "usage: MEMCORE_METRICS_API_KEY=... $0 <base-url>" >&2
  echo "  Optional: MEMCORE_METRICS_PATH=/metrics MEMCORE_METRICS_STRICT=true" >&2
  exit 1
fi

BASE_URL="${BASE_URL%/}"
METRICS_PATH="${MEMCORE_METRICS_PATH:-/metrics}"
case "$METRICS_PATH" in
  /*) ;;
  *) METRICS_PATH="/${METRICS_PATH}" ;;
esac
STRICT="${MEMCORE_METRICS_STRICT:-false}"
API_KEY="${MEMCORE_METRICS_API_KEY:-${MEMCORE_SMOKE_TEST_API_KEY:-}}"

if ! command -v curl >/dev/null 2>&1; then
  echo "error: curl is required" >&2
  exit 1
fi

fail() {
  echo "error: $*" >&2
  exit 2
}

soft_fail() {
  if [[ "$STRICT" == "true" ]]; then
    fail "$@"
  fi
  echo "warning: $*" >&2
  echo "check_metrics: metrics unavailable (non-strict); exiting 0"
  exit 0
}

URL="${BASE_URL}${METRICS_PATH}"
body="$(mktemp)"
trap 'rm -f "$body"' EXIT

curl_args=(-sS -o "$body" -w '%{http_code}' "$URL")
if [[ -n "$API_KEY" ]]; then
  code="$(curl -sS -o "$body" -w '%{http_code}' \
    -H "Authorization: Bearer ${API_KEY}" \
    "$URL" || true)"
else
  code="$(curl -sS -o "$body" -w '%{http_code}' "$URL" || true)"
fi

if [[ -n "$API_KEY" ]]; then
  key_state="set"
else
  key_state="not set"
fi
echo "check_metrics: GET ${METRICS_PATH} -> HTTP ${code} (api key ${key_state})"

case "$code" in
  404)
    soft_fail "metrics endpoint returned 404 (likely MEMCORE_METRICS_ENABLED=false)"
    ;;
  401|403)
    if [[ -z "$API_KEY" ]]; then
      soft_fail "metrics requires auth; set MEMCORE_METRICS_API_KEY"
    fi
    fail "metrics auth failed with HTTP ${code}"
    ;;
  2*)
    ;;
  *)
    fail "unexpected metrics status HTTP ${code}"
    ;;
esac

text="$(cat "$body")"
lower="$(printf '%s' "$text" | tr '[:upper:]' '[:lower:]')"

# Expected metric names (presence checks; process-local counters may be zero).
missing=0
for metric in memcore_http_requests_total memcore_http_request_duration_seconds; do
  if ! grep -q "$metric" <<<"$text"; then
    echo "warning: missing expected metric name: $metric" >&2
    missing=$((missing + 1))
  else
    echo "ok: found ${metric}"
  fi
done

for optional in memcore_memory_create_total memcore_context_requests_total memcore_http_request_errors_total; do
  if grep -q "$optional" <<<"$text"; then
    echo "ok: found optional ${optional}"
  fi
done

if [[ "$missing" -gt 0 && "$STRICT" == "true" ]]; then
  fail "strict mode: missing ${missing} required metric name(s)"
fi

# Secret / content safety (never print the API key itself).
if [[ -n "$API_KEY" ]] && grep -Fq "$API_KEY" <<<"$text"; then
  fail "metrics body contains the configured API key"
fi
for needle in "bearer " "postgres://" "redis://" "sk-live" "sk-proj-" "smoke test memory: user likes green tea"; do
  if grep -Fq "$needle" <<<"$lower"; then
    fail "metrics body contains forbidden pattern: ${needle}"
  fi
done

echo "check_metrics: passed against ${BASE_URL}"
exit 0
