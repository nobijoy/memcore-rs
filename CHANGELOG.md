# Changelog

All notable changes to memcore are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

When opening a PR, add a short note under **Unreleased**. Release managers move those
entries under a version heading when cutting a tag (see `docs/RELEASES.md`).

## Unreleased

### Added

- CI security checks (`cargo fmt` / clippy / test / audit / deny / gitleaks)
- Docker image security scan workflow (Trivy) and Syft SPDX SBOM artifacts
- Non-root Docker runtime user and OCI image labels
- Public `GET /api/v1/version` build metadata endpoint
- Manual draft GitHub release workflow foundation

### Changed

- Workspace crates share `[workspace.package]` version `0.1.0`

### Fixed

### Security

- API secret redaction and security headers / request hardening
- Container scan policy: fail on CRITICAL vulnerabilities
