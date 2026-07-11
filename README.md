# memcore

Production-oriented long-term memory engine for AI agents (Rust).

## Quick links

| Topic | Link |
|-------|------|
| Client quickstart | [docs/CLIENT_QUICKSTART.md](docs/CLIENT_QUICKSTART.md) |
| API overview | [docs/API.md](docs/API.md) |
| API examples (curl) | [docs/API_EXAMPLES.md](docs/API_EXAMPLES.md) |
| API errors | [docs/API_ERRORS.md](docs/API_ERRORS.md) |
| OpenAPI | [docs/OPENAPI.md](docs/OPENAPI.md) · `/docs` · `/openapi.json` |
| curl examples | [examples/curl/](examples/curl/) |
| Python example | [examples/python/](examples/python/) |
| Node example | [examples/node/](examples/node/) |
| Smoke tests | [docs/SMOKE_TESTS.md](docs/SMOKE_TESTS.md) |
| Deployment | [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) |
| Security | [docs/SECURITY.md](docs/SECURITY.md) |

## Develop

```bash
cargo check
cargo test -p memcore-common
cargo test -p memcore-config
cargo test -p memcore-core
cargo test -p memcore-storage
cargo test -p memcore-providers
cargo test -p memcore-api
```

Do not enable LanceDB for normal day-to-day tests (`docs/CURSOR_RULES.md`).

## Client examples

Examples use env vars (`MEMCORE_BASE_URL`, `MEMCORE_API_KEY`, `MEMCORE_ORG_ID`) and never print API keys. They are examples only — no npm/PyPI/crates.io SDK packages are published yet (`docs/SDK_ROADMAP.md`).
