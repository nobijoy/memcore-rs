# Local development image: SQLite + mock vector/providers (no LanceDB by default).
# Optional LanceDB build:
#   docker build -f docker/Dockerfile --build-arg CARGO_FEATURES=lancedb -t memcore-api:lancedb .

ARG CARGO_FEATURES=

FROM rust:bookworm AS builder
WORKDIR /app

# Copy workspace manifests first for better layer caching.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

ARG CARGO_FEATURES
RUN if [ -n "$CARGO_FEATURES" ]; then \
      cargo build --release -p memcore-api --features "$CARGO_FEATURES"; \
    else \
      cargo build --release -p memcore-api; \
    fi

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
RUN mkdir -p /app/data

COPY --from=builder /app/target/release/memcore-api /usr/local/bin/memcore-api

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=5s --retries=5 --start-period=15s \
  CMD curl -f http://localhost:8080/health || exit 1

CMD ["memcore-api"]
