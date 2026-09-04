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
#   MEMCORE_K6_BIN     (optional absolute path to k6 / k6.exe)
#   MEMCORE_PERF_USE_DOCKER_K6=true  (force Docker grafana/k6)
#
# IMPORTANT: Load tests must use mock providers. Real providers are forbidden by
# default (API blocks X-Memcore-Test-Source: load-test unless explicitly allowed).
# Never prints the API key. Does not call forget-user / restore / import-export.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROFILE="${1:-smoke}"
PROFILE="$(echo "$PROFILE" | tr '[:upper:]' '[:lower:]')"

usage() {
  echo "usage: $0 <smoke|baseline|stress>" >&2
  echo "  Requires k6 on PATH, MEMCORE_K6_BIN, or Docker (grafana/k6)." >&2
  echo "  Default base URL: http://localhost:8080" >&2
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

resolve_k6() {
  if [[ -n "${MEMCORE_K6_BIN:-}" && ( -x "${MEMCORE_K6_BIN}" || -f "${MEMCORE_K6_BIN}" ) ]]; then
    echo "${MEMCORE_K6_BIN}"
    return 0
  fi
  if command -v k6 >/dev/null 2>&1; then
    local resolved
    resolved="$(command -v k6)"
    # Prefer a native Linux k6 under WSL; Windows k6.exe cannot open /mnt/c paths.
    if [[ "$(uname -s)" == "Linux" && "$resolved" == *.exe ]]; then
      return 1
    fi
    echo "$resolved"
    return 0
  fi
  # Git Bash on Windows (not WSL): allow k6.exe with Windows-style paths.
  if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* || "$(uname -s)" == CYGWIN* ]]; then
    local candidates=(
      "/c/Program Files/k6/k6.exe"
      "C:/Program Files/k6/k6.exe"
    )
    local candidate
    for candidate in "${candidates[@]}"; do
      if [[ -f "$candidate" ]]; then
        echo "$candidate"
        return 0
      fi
    done
  fi
  return 1
}

# Convert /mnt/c/... -> C:/... when invoking Windows k6.exe from WSL/Git Bash.
to_k6_path() {
  local path="$1"
  if [[ "$path" == /mnt/[a-zA-Z]/* ]]; then
    local drive
    drive="$(echo "${path:5:1}" | tr '[:lower:]' '[:upper:]')"
    echo "${drive}:${path:6}"
    return 0
  fi
  if [[ "$path" == /[a-zA-Z]/* ]]; then
    local drive
    drive="$(echo "${path:1:1}" | tr '[:lower:]' '[:upper:]')"
    echo "${drive}:${path:2}"
    return 0
  fi
  echo "$path"
}

K6_BIN=""
USE_DOCKER_K6=0
if [[ "${MEMCORE_PERF_USE_DOCKER_K6:-}" == "true" ]]; then
  USE_DOCKER_K6=1
elif K6_BIN="$(resolve_k6)"; then
  USE_DOCKER_K6=0
elif command -v docker >/dev/null 2>&1; then
  USE_DOCKER_K6=1
  echo "memcore perf: local k6 not on PATH; using Docker image grafana/k6"
else
  echo "error: k6 is not installed or on PATH, and Docker is unavailable" >&2
  echo "  Install: https://grafana.com/docs/k6/latest/set-up/install-k6/" >&2
  echo "  Or set MEMCORE_K6_BIN to k6.exe, or enable Docker for grafana/k6 fallback" >&2
  exit 1
fi

BASE_URL="${MEMCORE_BASE_URL:-http://localhost:8080}"
BASE_URL="${BASE_URL%/}"
ORG_ID="${MEMCORE_ORG_ID:-${MEMCORE_SMOKE_TEST_ORG_ID:-org_perf}}"
# Prefer explicit load-test key; fall back to smoke-test key (never printed).
API_KEY="${MEMCORE_API_KEY:-${MEMCORE_SMOKE_TEST_API_KEY:-}}"
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
if [[ "$USE_DOCKER_K6" -eq 1 ]]; then
  echo "memcore perf: runner=docker(grafana/k6)"
else
  echo "memcore perf: runner=local-k6"
fi
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

run_local_k6() {
  local script_path="$SCRIPT"
  local json_out="$JSON_OUT"
  local workdir="$ROOT_DIR"
  if [[ "$K6_BIN" == *.exe ]]; then
    script_path="$(to_k6_path "$SCRIPT")"
    json_out="$(to_k6_path "$JSON_OUT")"
    workdir="$(to_k6_path "$ROOT_DIR")"
    cd "$ROOT_DIR"
  fi
  (
    cd "$ROOT_DIR"
    "$K6_BIN" run \
      --summary-export "$json_out" \
      "$script_path"
  )
}

run_docker_k6() {
  # Docker Desktop on Windows/macOS cannot use --network host reliably.
  # Map localhost targets to host.docker.internal for container access.
  local docker_base="$BASE_URL"
  if [[ "$docker_base" == http://localhost:* ]] || [[ "$docker_base" == http://127.0.0.1:* ]]; then
    docker_base="${docker_base/http:\/\/localhost/http:\/\/host.docker.internal}"
    docker_base="${docker_base/http:\/\/127.0.0.1/http:\/\/host.docker.internal}"
  fi

  local -a docker_env=(
    -e "MEMCORE_BASE_URL=${docker_base}"
    -e "MEMCORE_ORG_ID=${ORG_ID}"
    -e "MEMCORE_TEST_PROFILE=${PROFILE}"
    -e "MEMCORE_PERF_RUN_ID=${RUN_ID}"
  )
  if [[ -n "${API_KEY}" ]]; then
    docker_env+=(-e "MEMCORE_API_KEY=${API_KEY}")
  fi
  if [[ -n "${MEMCORE_ALLOW_STRESS_TEST:-}" ]]; then
    docker_env+=(-e "MEMCORE_ALLOW_STRESS_TEST=${MEMCORE_ALLOW_STRESS_TEST}")
  fi

  docker run --rm \
    --add-host=host.docker.internal:host-gateway \
    "${docker_env[@]}" \
    -v "${ROOT_DIR}/scripts/perf/k6:/scripts:ro" \
    -v "${REPORT_DIR}:/home/k6/reports/perf" \
    -w /home/k6 \
    grafana/k6:latest \
    run --summary-export "/home/k6/reports/perf/memcore-${PROFILE}-${RUN_ID}.json" \
    /scripts/memcore_load.js
}

set +e
if [[ "$USE_DOCKER_K6" -eq 1 ]]; then
  run_docker_k6 | tee "$TXT_OUT"
  STATUS=${PIPESTATUS[0]}
else
  run_local_k6 | tee "$TXT_OUT"
  STATUS=${PIPESTATUS[0]}
fi
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
