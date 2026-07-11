# Grafana dashboard templates

Vendor-neutral **templates** for memcore. They are not deployed by CI and do not require a paid Grafana Cloud plan — import into any Grafana (or adapt PromQL to another UI).

## Files

| File | Purpose |
|------|---------|
| `grafana-memcore-overview.json` | Service health, RPS, errors, latency, core ops |
| `grafana-memcore-api.json` | Routes, 4xx/5xx, auth, rate limits |
| `grafana-memcore-background-jobs.json` | Job runs, failures, duration, skips |
| `grafana-memcore-providers.json` | Provider traffic, latency, failures, retries |

## Import

1. Run Prometheus (or compatible) scraping `GET /metrics` on each memcore replica when `MEMCORE_METRICS_ENABLED=true`.
2. In Grafana: **Dashboards → Import → Upload JSON**.
3. Select your Prometheus datasource when prompted (`DS_PROMETHEUS` variable).

Many panels reference **intended** metric names from `docs/METRICS.md`. Until the Prometheus metrics foundation expands emission, expect empty series for labeled/histogram panels. Overview panels using implemented counters should work sooner.

## Safety

- No real datasource UIDs, API keys, or hostnames committed
- No panels that query raw logs for memory content/prompts
- Prefer `route` / `job_kind` / `provider` labels — never user text

## Validate locally

```bash
./scripts/ops/validate_dashboards.sh
```

## Related

- `docs/OBSERVABILITY.md`
- `docs/METRICS.md`
- `docs/ALERTING.md`
- `docs/SLO.md`
