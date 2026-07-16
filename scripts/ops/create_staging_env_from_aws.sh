#!/usr/bin/env bash
# Load staging env keys from AWS Secrets Manager into the environment, then
# generate a local gitignored .env.staging via create_staging_env.sh.
# Never prints secret values.
#
# Usage:
#   export MEMCORE_AWS_SECRET_ID='memcore/staging/env'   # placeholder — use real id privately
#   ./scripts/ops/create_staging_env_from_aws.sh
#
# Requires: aws CLI, jq
# See docs/runbooks/STAGING_AWS_SECRETS_MANAGER.md

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

fail() {
  echo "error: $*" >&2
  exit 1
}

if [[ -z "${MEMCORE_AWS_SECRET_ID:-}" ]]; then
  fail "MEMCORE_AWS_SECRET_ID is required (placeholder example: memcore/staging/env)"
fi

if ! command -v aws >/dev/null 2>&1; then
  fail "aws CLI is not installed"
fi

if ! command -v jq >/dev/null 2>&1; then
  fail "jq is required to parse SecretString JSON without printing values"
fi

echo "create_staging_env_from_aws: fetching secret id (value not printed)…"

secret_json="$(mktemp)"
trap 'rm -f "$secret_json"' EXIT

if ! aws secretsmanager get-secret-value \
  --secret-id "$MEMCORE_AWS_SECRET_ID" \
  --query SecretString \
  --output text >"$secret_json" 2>/dev/null; then
  fail "failed to fetch secret (check IAM, region, and MEMCORE_AWS_SECRET_ID) — no values printed"
fi

# Export each top-level JSON string/number as env without echoing values.
while IFS= read -r key; do
  [[ -z "$key" ]] && continue
  val="$(jq -r --arg k "$key" '.[$k] | tostring' "$secret_json")"
  if [[ "$val" == "null" ]]; then
    continue
  fi
  export "$key=$val"
done < <(jq -r 'keys[]' "$secret_json")

rm -f "$secret_json"
trap - EXIT

echo "create_staging_env_from_aws: keys exported (values not printed); calling create_staging_env.sh"
exec ./scripts/ops/create_staging_env.sh
