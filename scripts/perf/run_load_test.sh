#!/usr/bin/env bash
# Run memcore k6 load profiles and write summaries under reports/perf/.
#
# Usage:
#   ./scripts/perf/run_load_test.sh smoke
#   ./scripts/perf/run_load_test.sh baseline
#   MEMCORE_ALLOW_STRESS_TEST=true ./scripts/perf/run_load_test.sh stress
#
# Environment:
#   MEMCORE_BASE_URL   (default http://localhost:8080)
#   MEMCORE_API_KEY    (optional; enables authenticated memory flow)
#   MEMCORE_ORG_ID     (default org_perf)
#   MEMCORE_ALLOW_STRESS_TEST=true  (required for stress)
#
# Never prints the API key. Does not call forget-user / restore / import-export.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROFILE="${1:-smoke}"
PROFILE="$(echo "$PROFILE" | tr '[:upper:]' '[:lower:]')"

usage() {
  echo "usage: $0 <smoke|baseline|stress>" >&2
  echo "  Requires k6 on PATH. Default base URL: http://localhost:8080" >&2
  echo "  Stress requires MEMCORE_ALLOW_STRESS_TEST=true" >&2
  exit 1
}

case "$PROFILE" in
  smoke|baseline|stress) ;;
  -h|--help) usage ;;
  *)
    echo "error: unknown profile '$PROFILE'" >&2
    usage
    ;;
esac

if ! command -v k6 >/dev/null 2>&1; then
  echo "error: k6 is not installed or not on PATH" >&2
  echo "  Install: https://grafana.com/docs/k6/latest/set-up/install-k6/" >&2
  exit 1
fi

BASE_URL="${MEMCORE_BASE_URL:-http://localhost:8080}"
BASE_URL="${BASE_URL%/}"
ORG_ID="${MEMCORE_ORG_ID:-org_perf}"
API_KEY="${MEMCORE_API_KEY:-}"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
REPORT_DIR="${ROOT_DIR}/reports/perf"
SCRIPT="${ROOT_DIR}/scripts/perf/k6/memcore_load.js"

mkdir -p "$REPORT_DIR"

if [[ "$PROFILE" == "stress" ]]; then
  if [[ "${MEMCORE_ALLOW_STRESS_TEST:-}" != "true" ]]; then
    echo "error: stress profile requires MEMCORE_ALLOW_STRESS_TEST=true" >&2
    echo "  Refusing to start. Target would have been: $BASE_URL" >&2
    exit 1
  fi
fi

AUTH_STATE="disabled"
if [[ -n "$API_KEY" ]]; then
  AUTH_STATE="enabled"
fi

echo "memcore perf: profile=$PROFILE base=$BASE_URL org=$ORG_ID auth=$AUTH_STATE run_id=$RUN_ID"
echo "memcore perf: writing reports under reports/perf/ (API key never printed)"

export MEMCORE_BASE_URL="$BASE_URL"
export MEMCORE_ORG_ID="$ORG_ID"
export MEMCORE_TEST_PROFILE="$PROFILE"
export MEMCORE_PERF_RUN_ID="$RUN_ID"
# Pass through API key and stress guard without echoing them.
if [[ -n "$API_KEY" ]]; then
  export MEMCORE_API_KEY
fi
if [[ -n "${MEMCORE_ALLOW_STRESS_TEST:-}" ]]; then
  export MEMCORE_ALLOW_STRESS_TEST
fi

JSON_OUT="${REPORT_DIR}/memcore-${PROFILE}-${RUN_ID}.json"
TXT_OUT="${REPORT_DIR}/memcore-${PROFILE}-${RUN_ID}.txt"
LAST_JSON="${REPORT_DIR}/last-summary.json"

# Run from repo root so k6 handleSummary relative paths resolve under reports/perf/.
cd "$ROOT_DIR"

set +e
k6 run \
  --summary-export "$JSON_OUT" \
  "$SCRIPT" | tee "$TXT_OUT"
STATUS=$?
set -e

# Prefer k6 handleSummary last-summary.json when present; keep summary-export either way.
if [[ -f "$LAST_JSON" ]]; then
  cp "$LAST_JSON" "${REPORT_DIR}/memcore-${PROFILE}-${RUN_ID}-summary.json"
fi

echo "memcore perf: summary export -> $JSON_OUT"
echo "memcore perf: console log    -> $TXT_OUT"

if [[ "$STATUS" -ne 0 ]]; then
  echo "error: k6 exited with status $STATUS" >&2
  exit "$STATUS"
fi

echo "memcore perf: passed profile=$PROFILE"
