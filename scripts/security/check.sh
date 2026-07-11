#!/usr/bin/env bash
# Local security and quality checks for memcore.
# Prerequisites (optional tools):
#   - rustup stable with rustfmt + clippy
#   - cargo-audit  (cargo install cargo-audit --locked)
#   - cargo-deny   (cargo install cargo-deny --locked)
#   - gitleaks     (https://github.com/gitleaks/gitleaks)
#
# Usage:
#   ./scripts/security/check.sh
#   SKIP_AUDIT=1 SKIP_GITLEAKS=1 ./scripts/security/check.sh

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

if [[ "${SKIP_FEATURE_CHECKS:-0}" != "1" ]]; then
  run cargo check -p memcore-storage --features postgres
  run cargo check -p memcore-api --features postgres
  run cargo check -p memcore-storage --features redis-cache
  run cargo check -p memcore-api --features redis-cache
  run cargo check -p memcore-storage --features qdrant
  run cargo check -p memcore-api --features qdrant
fi

if [[ "${SKIP_AUDIT:-0}" != "1" ]]; then
  if command -v cargo-audit >/dev/null 2>&1 || cargo audit -V >/dev/null 2>&1; then
    run cargo audit
  else
    echo "warning: cargo-audit not installed; skip (cargo install cargo-audit --locked)" >&2
  fi

  if command -v cargo-deny >/dev/null 2>&1 || cargo deny --version >/dev/null 2>&1; then
    run cargo deny check
  else
    echo "warning: cargo-deny not installed; skip (cargo install cargo-deny --locked)" >&2
  fi
fi

if [[ "${SKIP_GITLEAKS:-0}" != "1" ]]; then
  if command -v gitleaks >/dev/null 2>&1; then
    run gitleaks detect --source . --no-git --redact --config .gitleaks.toml
  else
    echo "warning: gitleaks not installed; skip" >&2
  fi
fi

echo "All requested security checks passed."
