# Multi-stage image for memcore-api (no LanceDB or Qdrant by default).
# Optional features via CARGO_FEATURES build arg (comma-separated):
#   postgres  — required for MEMCORE_FACT_BACKEND=postgres (production compose)
#   qdrant    — required for MEMCORE_VECTOR_BACKEND=qdrant (production compose)
#   lancedb   — optional embedded vector backend (heavier build)
# Examples:
#   docker build -f docker/Dockerfile --build-arg CARGO_FEATURES=postgres -t memcore-api:postgres .
#   docker build -f docker/Dockerfile --build-arg CARGO_FEATURES="postgres,qdrant" -t memcore-api:prod .
#   docker build -f docker/Dockerfile --build-arg APP_VERSION=0.1.0 --build-arg GIT_SHA=abc123 -t memcore:0.1.0 .
#
# Security notes: runtime runs as non-root `memcore`; do not bake secrets into the image.
# See docs/CONTAINER_SECURITY.md and docs/RELEASES.md.

ARG CARGO_FEATURES=
ARG APP_VERSION=unknown
ARG GIT_SHA=unknown
ARG BUILD_TIMESTAMP=unknown

FROM rust:bookworm AS builder
WORKDIR /app

# Copy workspace manifests first for better layer caching.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

ARG CARGO_FEATURES
ARG APP_VERSION
ARG GIT_SHA
ARG BUILD_TIMESTAMP

ENV MEMCORE_BUILD_GIT_SHA=${GIT_SHA}
ENV MEMCORE_BUILD_TIMESTAMP=${BUILD_TIMESTAMP}
ENV MEMCORE_BUILD_PROFILE=release

RUN if [ -n "$CARGO_FEATURES" ]; then \
      cargo build --release -p memcore-api --features "$CARGO_FEATURES"; \
    else \
      cargo build --release -p memcore-api; \
    fi

FROM debian:bookworm-slim AS runtime

ARG APP_VERSION=unknown
ARG GIT_SHA=unknown
ARG BUILD_TIMESTAMP=unknown

LABEL org.opencontainers.image.title="memcore" \
      org.opencontainers.image.description="Rust-based long-term memory engine" \
      org.opencontainers.image.source="https://github.com/nobijoy/memcore-rs" \
      org.opencontainers.image.version="${APP_VERSION}" \
      org.opencontainers.image.revision="${GIT_SHA}" \
      org.opencontainers.image.created="${BUILD_TIMESTAMP}"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 memcore \
    && useradd --system --uid 10001 --gid memcore --home-dir /app --shell /usr/sbin/nologin memcore

WORKDIR /app
RUN mkdir -p /app/data \
    && chown -R memcore:memcore /app

COPY --from=builder /app/target/release/memcore-api /usr/local/bin/memcore-api

USER memcore:memcore

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=5s --retries=5 --start-period=15s \
  CMD curl -f http://localhost:8080/health || exit 1

CMD ["memcore-api"]
