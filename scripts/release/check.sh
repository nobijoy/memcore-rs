#!/usr/bin/env bash
# Pre-release validation for memcore.
#
# Usage (from repository root):
#   ./scripts/release/check.sh
#
# Optional skips:
#   SKIP_AUDIT=1 SKIP_DOCKER=1 ./scripts/release/check.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

run() {
  echo "==> $*"
  "$@"
}

run cargo fmt --all -- --check
run cargo clippy --workspace --all-targets -- -D warnings
run cargo test --workspace
run cargo build --release -p memcore-api

if [[ "${SKIP_AUDIT:-0}" != "1" ]]; then
  if cargo audit -V >/dev/null 2>&1; then
    run cargo audit
  else
    echo "warning: cargo-audit not installed; skip" >&2
  fi
  if cargo deny --version >/dev/null 2>&1; then
    run cargo deny check
  else
    echo "warning: cargo-deny not installed; skip" >&2
  fi
fi

if [[ "${SKIP_DOCKER:-0}" != "1" ]]; then
  if command -v docker >/dev/null 2>&1; then
    run docker build -f docker/Dockerfile -t memcore:release-check .
  else
    echo "warning: docker not installed; skip image build" >&2
  fi
fi

echo "Release checks passed."
