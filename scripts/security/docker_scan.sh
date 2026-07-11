#!/usr/bin/env bash
# Local Docker image build, vulnerability scan, and SBOM generation.
#
# Prerequisites:
#   - Docker
#   - Trivy  (https://aquasecurity.github.io/trivy/)
#   - Syft   (https://github.com/anchore/syft)
#
# Usage (from repository root):
#   ./scripts/security/docker_scan.sh
#
# Optional:
#   IMAGE_TAG=memcore:local ./scripts/security/docker_scan.sh
#   SKIP_BUILD=1 ./scripts/security/docker_scan.sh   # scan existing tag only

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

IMAGE_TAG="${IMAGE_TAG:-memcore:local}"
DOCKERFILE="${DOCKERFILE:-docker/Dockerfile}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: '$1' is required but not installed." >&2
    echo "  Install it, then re-run ./scripts/security/docker_scan.sh" >&2
    exit 1
  fi
}

need docker

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  echo "==> docker build -f ${DOCKERFILE} -t ${IMAGE_TAG} ."
  docker build -f "${DOCKERFILE}" -t "${IMAGE_TAG}" .
else
  echo "==> SKIP_BUILD=1; using existing image ${IMAGE_TAG}"
fi

if command -v trivy >/dev/null 2>&1; then
  echo "==> trivy image --severity HIGH,CRITICAL ${IMAGE_TAG}"
  # Local script reports HIGH+CRITICAL. CI fails only on CRITICAL (see docs/CONTAINER_SECURITY.md).
  trivy image --severity HIGH,CRITICAL "${IMAGE_TAG}"
else
  echo "warning: trivy not installed; skip image scan" >&2
fi

if command -v syft >/dev/null 2>&1; then
  echo "==> syft ${IMAGE_TAG} -o spdx-json=sbom.spdx.json"
  syft "${IMAGE_TAG}" -o spdx-json=sbom.spdx.json
  echo "Wrote sbom.spdx.json"
else
  echo "warning: syft not installed; skip SBOM generation" >&2
fi

echo "Docker security local checks finished for ${IMAGE_TAG}."
