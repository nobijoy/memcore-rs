# Grafana dashboard templates

Vendor-neutral **templates** for memcore. They are not deployed by CI and do not require a paid Grafana Cloud plan — import into any Grafana (or adapt PromQL to another UI).

## Files

| File | Purpose |
|------|---------|
| `grafana-memcore-overview.json` | Service health, RPS, errors, latency, core ops |
| `grafana-memcore-api.json` | Routes, 4xx/5xx, auth, rate limits |
| `grafana-memcore-background-jobs.json` | Job runs, failures, duration, lock skips |
| `grafana-memcore-providers.json` | Provider traffic, failures, retries, cache |

## Import

1. Enable scrape: `MEMCORE_METRICS_ENABLED=true` (keep `MEMCORE_METRICS_REQUIRE_AUTH=true` unless private-network-only).
2. Point Prometheus at each replica’s metrics path (default `/metrics`).
3. In Grafana: **Dashboards → Import → Upload JSON**.
4. Select your Prometheus datasource when prompted (`DS_PROMETHEUS` variable).

Panels use **implemented** metric names from `docs/METRICS.md` (e.g. `memcore_http_request_duration_seconds`, `memcore_memory_create_total`). Some provider duration series may be empty until latency is wired into usage events.

## Safety

- No real datasource UIDs, API keys, or hostnames committed
- No panels that query raw logs for memory content/prompts
- Prefer `route` / `job_kind` / `provider` labels — never user text

## Validate locally

```bash
./scripts/ops/validate_dashboards.sh
# or:
python -m json.tool dashboards/grafana-memcore-overview.json >/dev/null
```

## Related

- `docs/OBSERVABILITY.md`
- `docs/METRICS.md`
- `docs/ALERTING.md`
- `docs/SLO.md`
