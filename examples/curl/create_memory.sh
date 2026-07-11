#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=_common.sh
source "${SCRIPT_DIR}/_common.sh"
require_common_env

curl -sS -X POST "${MEMCORE_BASE_URL}/api/v1/memories" \
  -H "Authorization: Bearer ${MEMCORE_API_KEY}" \
  -H "X-Organization-ID: ${MEMCORE_ORG_ID}" \
  -H "Content-Type: application/json" \
  -d "{
    \"user_id\": \"${MEMCORE_USER_ID}\",
    \"messages\": [
      {\"role\": \"user\", \"content\": \"User prefers concise technical summaries.\"}
    ],
    \"metadata\": {\"source\": \"examples/curl\"}
  }"
echo
