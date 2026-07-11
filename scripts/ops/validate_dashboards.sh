#!/usr/bin/env bash
# Validate Grafana dashboard JSON templates (no Grafana required).
#
# Usage:
#   ./scripts/ops/validate_dashboards.sh

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DASH_DIR="${ROOT_DIR}/dashboards"

if [[ ! -d "$DASH_DIR" ]]; then
  echo "error: dashboards directory missing: $DASH_DIR" >&2
  exit 1
fi

shopt -s nullglob
files=("$DASH_DIR"/grafana-memcore-*.json)
if [[ ${#files[@]} -eq 0 ]]; then
  echo "error: no grafana-memcore-*.json files in $DASH_DIR" >&2
  exit 1
fi

validate_one() {
  local file="$1"
  if command -v jq >/dev/null 2>&1; then
    jq -e . >/dev/null < "$file"
  elif command -v python >/dev/null 2>&1; then
    python -m json.tool "$file" >/dev/null
  elif command -v python3 >/dev/null 2>&1; then
    python3 -m json.tool "$file" >/dev/null
  else
    echo "error: need jq or python to validate JSON" >&2
    exit 1
  fi
  echo "ok: $(basename "$file")"
}

for f in "${files[@]}"; do
  validate_one "$f"
done

echo "validate_dashboards: passed (${#files[@]} files)"
