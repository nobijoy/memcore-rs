# memcore

**A production-oriented, multi-tenant long-term memory engine for AI agents and applications, written in Rust.**

memcore sits between your LLM application and its state: instead of replaying an ever-growing chat transcript into every prompt, memcore extracts durable *facts* from interactions, resolves conflicts between old and new facts, and serves back a ranked, budget-aware context on demand — over a REST API, with the tenancy, security, and operational controls a real backend service needs.

[![CI](https://github.com/nobijoy/memcore-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/nobijoy/memcore-rs/actions/workflows/ci.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

## Table of contents

- [Problem statement](#problem-statement)
- [Core concepts](#core-concepts)
- [System architecture](#system-architecture)
- [Request lifecycle](#request-lifecycle)
- [Data & storage architecture](#data--storage-architecture)
- [Provider architecture](#provider-architecture)
- [API surface](#api-surface)
- [Non-functional characteristics](#non-functional-characteristics)
- [Tech stack](#tech-stack)
- [Getting started](#getting-started)
- [Configuration](#configuration)
- [Testing & quality gates](#testing--quality-gates)
- [Deployment topologies](#deployment-topologies)
- [Known limitations & roadmap](#known-limitations--roadmap)
- [Security](#security)
- [Contributing](#contributing)
- [License](#license)

## Problem statement

Naively stuffing conversation history into an LLM context window doesn't scale: it's expensive, it degrades retrieval quality as history grows, it has no notion of *contradicting* or *superseding* information, and it gives you no durable, queryable record of what the system actually believes about a user. memcore addresses this by treating memory as a first-class, versioned data problem rather than a prompt-engineering problem:

- Extract atomic **facts** from raw interactions instead of storing raw transcripts as the source of truth.
- Resolve each new fact against existing memory (**add**, **update**, **delete**, or **no-op**) instead of blindly appending.
- Retrieve with **semantic similarity + temporal relevance**, not just cosine-nearest-neighbor.
- Isolate every fact by **organization and user** so the system is safe to run as shared infrastructure.
- Treat **cost, security, and observability** as core requirements, not afterthoughts bolted on before shipping.

## Core concepts

| Concept | Description |
|---|---|
| **Fact** | An atomic, durable unit of memory (e.g. "user prefers concise answers"), with confidence/importance scores, tenant scoping, and an audit trail. |
| **Event** | An immutable log entry recording *why* a fact changed — the append-only audit layer behind mutable fact state. |
| **Memory lifecycle** | Every incoming interaction produces zero or more lifecycle operations against existing facts: **Add** (new fact), **Update** (supersede a conflicting fact), **Delete** (retract a fact), **NoOp** (already known). |
| **Context assembly** | At read time, candidate facts are retrieved by vector similarity, then re-ranked by recency/temporal signals and packed under a token/size budget before being returned to the caller. |
| **Tenant** | Every fact, event, and API call is scoped by `organization_id` + `user_id`. There is no cross-tenant read path. |
| **Provider** | The pluggable LLM (fact extraction, conflict classification) and embedding (vectorization) backends behind a trait boundary — mock, OpenAI, or any OpenAI-compatible endpoint. |

## System architecture

```text
                              ┌─────────────────────────┐
        HTTP/JSON             │        Client(s)         │
        ───────────▶          └─────────────┬─────────────┘
                                            │
                              ┌─────────────▼─────────────┐
                              │        Axum HTTP API        │
                              │  routes: memories · context  │
                              │  · users · admin · api-keys   │
                              └─────────────┬─────────────┘
                                            │
                  ┌─────────────────────────┼─────────────────────────┐
                  ▼                         ▼                         ▼
        ┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
        │  Middleware chain  │     │  Core orchestration │     │  Observability     │
        │  request id/log    │     │  extraction         │     │  structured logs    │
        │  security headers  │     │  conflict resolution │     │  Prometheus metrics │
        │  CORS (optional)   │     │  ranking / dedup     │     │  request tracing    │
        │  body-size / JSON  │     │  context assembly    │     │  admin/audit APIs   │
        │  auth (dev/db)     │     │  retention / privacy │     └──────────────────┘
        │  tenant extraction │     │  import / export     │
        │  rate limiting     │     │  background jobs     │
        └──────────────────┘     └─────────┬───────────┘
                                            │
                        ┌───────────────────┼───────────────────┐
                        ▼                                       ▼
              ┌───────────────────┐                   ┌───────────────────┐
              │   Storage ports     │                   │   Provider ports    │
              │  FactStore           │                   │  LlmProvider         │
              │  MemoryEventStore     │                   │  EmbeddingProvider    │
              │  VectorStore           │                   └─────────┬───────────┘
              └─────────┬───────────┘                             │
                        │                                          ▼
      ┌─────────────────┼─────────────────┐          ┌───────────────────────┐
      ▼                 ▼                 ▼          │  mock · OpenAI ·        │
  SQLite /          Qdrant / LanceDB   context cache   │  any OpenAI-compatible   │
  Postgres          / mock vectors     (memory/Redis)  │  endpoint (guardrailed)  │
```

The workspace is split into focused crates along these boundaries:

| Crate | Responsibility | Key internal modules |
|---|---|---|
| `memcore-api` | Axum HTTP server: routing, middleware, admin/observability endpoints, OpenAPI generation | `routes` (memories, context, users, admin, api-keys), `middleware` (auth, tenant, rate limit), `metrics` |
| `memcore-core` | Memory lifecycle orchestration and business logic — the domain layer | `engine` (lifecycle resolution), `context`, `ranking`, `dedup`, `importance`, `privacy`, `retention`, `import`/`export`, `jobs`, `audit`, `admin`, `org` |
| `memcore-storage` | Storage-port implementations behind trait boundaries | `sqlite`, `postgres`, `qdrant`, `lancedb`, `context_cache`, `keyword_search`, `migrations`, `backup` |
| `memcore-providers` | LLM/embedding provider trait implementations | OpenAI-compatible HTTP client, mock provider, cost-guardrail enforcement |
| `memcore-config` | Typed `Settings` loaded and validated from environment variables at process startup | fail-fast validation, secret-aware `Debug` redaction |
| `memcore-common` | Cross-cutting types shared by every crate | error types, the `Redactor` secret-scrubbing utility |

## Request lifecycle

A `POST` to create a memory flows through the system as follows:

1. **Ingress** — Axum receives the request; a request ID is assigned (or a caller-supplied one validated and reused).
2. **Cross-cutting middleware** — security headers are attached, CORS is evaluated (disabled by default), body size and `Content-Type` are validated.
3. **Auth** — the bearer API key is validated against either a single dev key or a hashed, scope-checked database-backed key, depending on auth mode.
4. **Tenant extraction** — `organization_id` (header) and `user_id` (path/body) are resolved and attached to the request context; every downstream call is scoped to this tenant.
5. **Rate limiting** — the request is checked against a per-organization, per-process token bucket.
6. **Extraction** — the message content is sent to the configured LLM provider to extract candidate facts (skipped entirely if the endpoint doesn't require it).
7. **Conflict resolution** — each candidate fact is compared against existing memory for the tenant; the engine decides Add / Update / Delete / NoOp per fact.
8. **Persistence** — accepted facts are written to the fact store (SQLite/Postgres) and an event is appended to the event store; if a vector backend is configured, an embedding is generated and upserted.
9. **Response** — a redacted, client-safe representation of the outcome is returned; internal errors are mapped to safe, generic messages (no SQL, URLs, or stack traces ever reach the client).

Read paths (`search`, `context`) skip extraction/conflict-resolution and instead: embed the query, retrieve nearest-neighbor candidates from the vector store, re-rank by recency/importance, and pack the result under a configured token/size budget.

## Data & storage architecture

memcore uses **trait-based storage ports** so the domain layer never depends on a concrete database:

| Port | Purpose | Implementations |
|---|---|---|
| `FactStore` | Durable fact CRUD, tenant-scoped queries, pagination | in-memory mock, SQLite, Postgres |
| `MemoryEventStore` | Append-only audit trail of lifecycle decisions | mock, SQLite, Postgres |
| `VectorStore` | Embedding upsert + nearest-neighbor search | in-memory mock, Qdrant (gRPC), LanceDB (embedded, optional/heavy) |
| `ApiKeyStore` | Hashed API key storage for database auth mode | mock, SQLite, Postgres |
| Context cache | Optional read-through cache for assembled context | in-process memory, Redis (feature-gated) |

Backend selection is a compile-time feature flag plus a runtime environment variable, so a given deployment only pays for the dependencies it actually uses (e.g. a Postgres+Qdrant production image doesn't link LanceDB). Postgres access goes through `sqlx` with compile-time-checked migrations; Qdrant access uses gRPC for the Rust client (REST is reserved for health checks only).

## Provider architecture

LLM and embedding access is abstracted behind `LlmProvider` and `EmbeddingProvider` traits with three implementations in practice:

1. **Mock** — deterministic, zero-cost, used for tests, CI, and heavy load testing.
2. **OpenAI** — the canonical implementation, talking to `api.openai.com`.
3. **OpenAI-compatible** — the *same* client pointed at a different base URL and key, which is how memcore talks to Gemini, Groq, Z.ai/GLM, and AWS Bedrock's OpenAI-compatible endpoint without a bespoke SDK per vendor.

Every real-provider call path is wrapped by a **cost-guardrail layer** that is independent of which vendor is configured:

- A test-mode switch (`mock_only` / `single_real` / `multi_real` / `production`) gates whether real network calls can happen at all.
- Per-run call count, input size, output tokens, retries, and timeout are all capped.
- Real-provider calls are automatically blocked when a request is tagged as a load test, and disabled by default for background jobs.
- An admin endpoint exposes live guardrail state (mode, caps, calls used/remaining) without ever exposing the underlying API key.

This means switching from a free-tier smoke test to a different vendor is a configuration change, not a code change — while making it structurally difficult to accidentally burn provider credits during load testing or CI.

## API surface

| Group | Examples |
|---|---|
| Health & meta | liveness/readiness probes, build/version info |
| Memories | create, list, delete a memory; per-user memory listing |
| Search & context | semantic search, budget-aware context assembly |
| Import / export | tenant-scoped export, import with dry-run validation |
| Retention | policy-driven fact/event expiry (dry-run by default) |
| Audit | memory-event history for a user |
| Admin | organization summary/user listing, provider guardrail status, provider usage history, background-job history, API-key management |
| Observability | Prometheus-compatible metrics scrape (auth-gated, disabled by default) |

Every mutating and read endpoint requires tenant scoping (`organization_id` + `user_id`); there is no endpoint that returns memory across tenant boundaries.

## Non-functional characteristics

- **Multi-tenancy** — enforced at the API and storage layer, not just by convention; covered by end-to-end tests.
- **Security posture** — bearer/API-key auth (single dev key or hashed database-backed keys with scopes), safe error messages, pattern-based secret redaction in logs/metrics/errors, security headers by default, CORS off by default, request body limits, no destructive restore path enabled by default.
- **Observability** — structured JSON logs with request IDs, Prometheus metrics behind auth, admin history APIs for provider usage and background jobs — all designed so prompts, memory content, and credentials never leak into telemetry.
- **Cost control** — provider guardrails as described above; rate limiting per organization.
- **Fail-fast configuration** — the process validates all environment-derived settings at startup and refuses to start on invalid config rather than degrading silently.
- **Known scaling boundary** — rate limiting and metrics are currently **per-process**, not aggregated across replicas; this is an explicit, documented limitation rather than an oversight, and is called out wherever multi-replica deployment is discussed.

## Tech stack

| Layer | Choice |
|---|---|
| Language | Rust (2024 edition) |
| HTTP framework | Axum, on Tokio |
| Relational storage | SQLite and PostgreSQL, via `sqlx` |
| Vector storage | Qdrant (gRPC client) and LanceDB (embedded, optional) |
| Optional cache | Redis (feature-gated) |
| LLM/embedding transport | `reqwest` (rustls), OpenAI-compatible protocol |
| Observability | `tracing`, `metrics` + Prometheus exporter |
| API docs | `utoipa` (OpenAPI generation) + Swagger UI |
| CI | GitHub Actions — fmt, clippy, workspace tests, feature matrix, `cargo audit`, `cargo deny`, gitleaks, Trivy image scan, SBOM generation |

## Getting started

Requirements: a recent Rust toolchain (2024 edition) and, for the containerized path, Docker.

**Run locally** (mock providers, SQLite, no external services):

```bash
cp .env.local.example .env
cargo run -p memcore-api
```

```bash
curl http://localhost:8080/health
curl http://localhost:8080/ready
```

**Run with Docker Compose** (Postgres + Qdrant, mock providers):

```bash
cp .env.local.example .env
docker compose -f docker/docker-compose.local.yml up --build -d
```

**Example authenticated request:**

```bash
curl -X POST http://localhost:8080/api/v1/memories \
  -H "Authorization: Bearer $MEMCORE_API_KEY" \
  -H "X-Organization-ID: org_demo" \
  -H "Content-Type: application/json" \
  -d '{
        "user_id": "user_123",
        "messages": [{"role": "user", "content": "I prefer concise, technical answers."}],
        "metadata": {"source": "chat"}
      }'
```

Interactive API documentation (Swagger UI) is served at `/docs`, with the machine-readable spec at `/openapi.json`, whenever the API is running.

## Configuration

memcore is configured entirely through environment variables, typed and validated at startup. Configuration is layered by environment (local, staging, production) via separate example env files that operators copy and fill in — real, secret-bearing env files are never committed. A dedicated validation script checks any env file for missing/inconsistent values, dangerous defaults (e.g. restore enabled, metrics unauthenticated), and provider-mode consistency before the service is allowed to start.

Broad configuration categories:

- **Runtime** — host/port, environment name, log format/level
- **Storage backends** — fact/event backend selection, database URL, migration mode
- **Vector backend** — backend selection, connection URL, collection name
- **Auth** — mode (dev single-key vs. database-backed scoped keys), key material
- **Providers** — LLM/embedding provider selection, API key/base URL, cost-guardrail caps
- **Security & ops** — rate limits, CORS, security headers, metrics auth, restore/backup toggles

## Testing & quality gates

```bash
cargo check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

cargo test -p memcore-common
cargo test -p memcore-config
cargo test -p memcore-core
cargo test -p memcore-storage
cargo test -p memcore-providers
cargo test -p memcore-api
```

Optional backends are compiled in only when needed:

```bash
cargo build -p memcore-api --features postgres,qdrant
```

The `lancedb` feature is heavy and reserved for manual/CI-only testing — it is intentionally excluded from the default day-to-day workflow.

CI runs on every push: formatting, clippy (warnings-as-errors), the full workspace test suite, a feature-flag compile matrix, `cargo audit` and `cargo deny` for dependency vulnerabilities/license policy, and gitleaks for secret scanning. Container images are additionally scanned with Trivy and published with a signed SBOM.

## Deployment topologies

memcore is designed to be deployed incrementally, with every stage explicitly validated before the next is attempted:

1. **Local** — `cargo run` or single-host Docker Compose, mock providers, SQLite.
2. **Local production-like** — Docker Compose with Postgres + Qdrant + mock providers, full smoke/metrics/backup/load-baseline validation.
3. **Cloud staging** — a single-instance Compose deployment (Postgres + Qdrant + memcore) behind a security group restricted to the operator's IP, validated with both mock and at least one real LLM provider under cost guardrails.
4. **Internal/private beta** — the staging deployment opened to a small set of trusted testers, with rotated secrets and manual backup/restore procedures, but no public SLA.
5. **Public beta** — requires TLS termination in front of the service, a public-facing secret rotation, and explicit operator sign-off.
6. **Full commercial production** — a managed database, automated backups, monitoring/alerting wired to paging, and a high-availability topology; intentionally out of scope for the current architecture, which is single-node by design.

At every stage, the project distinguishes between what has actually been validated end-to-end and what is merely architecturally supported — claims of readiness are never made ahead of evidence.

## Known limitations & roadmap

- Rate limiting and metrics are per-process; a shared/gateway rate limiter is needed before true multi-replica deployment.
- LanceDB support exists but is treated as optional/experimental and excluded from default CI.
- Native (non-OpenAI-compatible) SDKs for individual LLM vendors are not implemented; the OpenAI-compatible HTTP path is used uniformly instead.
- There is no Kubernetes/Helm/Terraform automation, no built-in TLS termination, and no managed-database integration — these are deliberately left to the operator's infrastructure of choice.
- Client libraries exist as reference examples only; no package is published to a language package registry yet.

## Security

Suspected security vulnerabilities should be reported privately to the maintainer rather than through a public issue. The project maintains an explicit hardening reference covering authentication, tenant isolation, secret redaction, security headers, CORS defaults, and safe metrics/log exposure, and treats "what is not yet hardened" as something to document plainly rather than omit.

## Contributing

Issues and pull requests are welcome. Before opening a PR:

1. Run the formatting, clippy, and full workspace test suite locally.
2. Keep the `lancedb` feature out of default day-to-day builds/tests.
3. Never commit secrets, real environment files, or generated reports.
4. Follow the project's existing conventions for redaction and safe error handling in any code that touches logs, metrics, or error responses.

## License

memcore is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0-or-later)**.

In short: you're free to use, modify, and self-host memcore, including commercially. If you modify memcore and let others interact with your modified version over a network (for example, offering it as a hosted service), the AGPL requires you to make the corresponding source code available to those users. If that doesn't fit your use case, contact the maintainer about alternative licensing.
