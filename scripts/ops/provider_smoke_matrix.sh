#!/usr/bin/env bash
# Tiny real-provider smoke helper (one provider per run).
#
# Does NOT call providers unless the running API is configured for them.
# This script only orchestrates smoke_test.sh with confirmation + reporting.
#
# Usage:
#   MEMCORE_PROVIDER_SMOKE_CONFIRM=I_UNDERSTAND_THIS_WILL_USE_PROVIDER_CREDITS \
#     ./scripts/ops/provider_smoke_matrix.sh gemini
#
# Supported names: gemini | groq | bedrock | openai
#
# Manual env updates are required before starting the API (do not put secrets here):
#   MEMCORE_PROVIDER_TEST_MODE=single_real
#   MEMCORE_REAL_PROVIDER_CALLS_ENABLED=true
#   MEMCORE_LLM_PROVIDER=openai
#   MEMCORE_EMBEDDING_PROVIDER=openai   # or mock if embeddings unsupported
#   OPENAI_API_KEY=...                  # vendor key / OpenAI-compat key
#   OPENAI_BASE_URL=...                 # vendor OpenAI-compat base URL
#   MEMCORE_BACKGROUND_JOBS_ENABLED=false
#   MEMCORE_PROVIDER_MAX_CALLS_PER_RUN=6
#   MEMCORE_PROVIDER_MAX_INPUT_CHARS=500
#   MEMCORE_PROVIDER_MAX_OUTPUT_TOKENS=256
#   MEMCORE_PROVIDER_MAX_RETRIES_PER_CALL=1
#
# Never prints API keys. Writes a redacted report under reports/provider-smoke/.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROVIDER="${1:-}"
CONFIRM_EXPECTED="I_UNDERSTAND_THIS_WILL_USE_PROVIDER_CREDITS"
BASE_URL="${MEMCORE_BASE_URL:-http://localhost:8080}"
BASE_URL="${BASE_URL%/}"

usage() {
  echo "usage: $0 <gemini|groq|bedrock|openai>" >&2
  echo "  Requires MEMCORE_PROVIDER_SMOKE_CONFIRM=${CONFIRM_EXPECTED}" >&2
  echo "  Configure the API env for one OpenAI-compatible provider before running." >&2
  exit 1
}

case "$PROVIDER" in
  gemini|groq|bedrock|openai) ;;
  -h|--help|"") usage ;;
  *)
    echo "error: unknown provider '$PROVIDER'" >&2
    usage
    ;;
esac

if [[ "${MEMCORE_PROVIDER_SMOKE_CONFIRM:-}" != "$CONFIRM_EXPECTED" ]]; then
  echo "error: set MEMCORE_PROVIDER_SMOKE_CONFIRM=${CONFIRM_EXPECTED}" >&2
  exit 1
fi

if [[ -z "${MEMCORE_SMOKE_TEST_API_KEY:-}" ]]; then
  echo "error: MEMCORE_SMOKE_TEST_API_KEY is required (value not printed)" >&2
  exit 1
fi

REPORT_DIR="${ROOT_DIR}/reports/provider-smoke"
mkdir -p "$REPORT_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
REPORT_FILE="${REPORT_DIR}/${PROVIDER}_${STAMP}.md"

echo "provider_smoke_matrix: provider=${PROVIDER} base=${BASE_URL}"
echo "provider_smoke_matrix: using synthetic smoke content via scripts/ops/smoke_test.sh"
echo "provider_smoke_matrix: ensure API is configured for this provider (manual env; secrets not printed)"

set +e
MEMCORE_SMOKE_TEST_EXPECT_REAL_PROVIDER=true \
  bash "${ROOT_DIR}/scripts/ops/smoke_test.sh" "$BASE_URL" --authenticated
SMOKE_RC=$?
set -e

{
  echo "# Provider Smoke Validation Report"
  echo
  echo "## Scope"
  echo "Single-provider micro-smoke via smoke_test.sh"
  echo
  echo "## Date"
  echo "$STAMP"
  echo
  echo "## Environment"
  echo "- base_url: ${BASE_URL}"
  echo "- provider_requested: ${PROVIDER}"
  echo "- expect_real_provider: true"
  echo "- api_key: set (not printed)"
  echo
  echo "## Provider"
  echo "$PROVIDER"
  echo
  echo "## Model"
  echo "(configured in API env; not printed by this script)"
  echo
  echo "## Mode"
  echo "single_real (operator must set MEMCORE_PROVIDER_TEST_MODE)"
  echo
  echo "## Call Limits"
  echo "Use MEMCORE_PROVIDER_MAX_CALLS_PER_RUN / INPUT_CHARS / OUTPUT_TOKENS on the API"
  echo
  echo "## Checks"
  echo
  echo "| Check | Result | Notes |"
  echo "|---|---|---|"
  echo "| Provider key present | (manual) | not printed |"
  echo "| Env validation | (manual) | |"
  if [[ "$SMOKE_RC" -eq 0 ]]; then
    echo "| Health/ready/version + authenticated smoke | Pass | |"
  else
    echo "| Health/ready/version + authenticated smoke | Fail | exit=${SMOKE_RC} |"
  fi
  echo "| Search | (included in smoke) | |"
  echo "| Context | (included in smoke) | |"
  echo "| Metrics | (manual follow-up) | |"
  echo "| Usage tracking | (manual follow-up) | |"
  echo "| Logs redaction | (manual spot-check) | |"
  echo "| Cost estimate | (manual) | |"
  echo
  echo "## Issues"
  if [[ "$SMOKE_RC" -ne 0 ]]; then
    echo "- smoke_test.sh failed with exit ${SMOKE_RC}"
  else
    echo "- none recorded by script"
  fi
  echo
  echo "## Decision"
  if [[ "$SMOKE_RC" -eq 0 ]]; then
    echo "Provisional Pass for ${PROVIDER} micro-smoke (operator must still review logs/metrics/cost)."
  else
    echo "Fail — do not promote; fix provider/config before retry."
  fi
} >"$REPORT_FILE"

echo "provider_smoke_matrix: wrote redacted report ${REPORT_FILE}"
exit "$SMOKE_RC"
